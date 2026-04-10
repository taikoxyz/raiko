use std::{collections::HashMap, hint::black_box};

use alloy_primitives::Address;
use alloy_rpc_types::EIP1186AccountProofResponse;
use interfaces::{cancel_proof, run_batch_prover, run_prover};
use raiko_lib::{
    builder::{create_mem_db, RethBlockBuilder},
    consts::ChainSpec,
    input::{GuestBatchInput, GuestBatchOutput, GuestInput, GuestOutput, TaikoProverData},
    protocol_instance::ProtocolInstance,
    prover::{IdStore, IdWrite, Proof, ProofKey},
    utils::txs::{generate_transactions, generate_transactions_for_batch_blocks},
};
use reth_primitives::{Block, Header};
use serde_json::Value;
use tracing::{debug, error, info, warn};

use crate::{
    interfaces::{
        run_shasta_proposal_prover, ProofRequest, RaikoError, RaikoResult, ShastaProposalCheckpoint,
    },
    preflight::{batch_preflight, BatchPreflightData},
    provider::BlockDataProvider,
};

pub mod interfaces;
pub mod preflight;
pub mod prover;
pub mod provider;

pub type MerkleProof = HashMap<Address, EIP1186AccountProofResponse>;

pub struct Raiko {
    pub l1_chain_spec: ChainSpec,
    pub taiko_chain_spec: ChainSpec,
    pub request: ProofRequest,
}

impl Raiko {
    pub fn new(
        l1_chain_spec: ChainSpec,
        taiko_chain_spec: ChainSpec,
        request: ProofRequest,
    ) -> Self {
        Self {
            l1_chain_spec,
            taiko_chain_spec,
            request,
        }
    }

    fn get_batch_preflight_data(&self) -> BatchPreflightData {
        BatchPreflightData {
            batch_id: self.request.batch_id,
            block_numbers: self.request.l2_block_numbers.clone(),
            l1_inclusion_block_number: self.request.l1_inclusion_block_number,
            l1_chain_spec: self.l1_chain_spec.clone(),
            taiko_chain_spec: self.taiko_chain_spec.clone(),
            prover_data: TaikoProverData {
                graffiti: self.request.graffiti,
                actual_prover: self.request.prover,
                checkpoint: self
                    .request
                    .checkpoint
                    .clone()
                    .map(ShastaProposalCheckpoint::into),
                last_anchor_block_number: self.request.last_anchor_block_number,
            },
            blob_proof_type: self.request.blob_proof_type.clone(),
            cached_event_data: self.request.cached_event_data.clone(),
        }
    }

    pub async fn generate_batch_input<BDP: BlockDataProvider>(
        &self,
        provider: BDP,
    ) -> RaikoResult<GuestBatchInput> {
        //TODO: read fork from config
        let preflight_data = self.get_batch_preflight_data();
        info!("Generating batch input for batch {}", self.request.batch_id);
        batch_preflight(provider, preflight_data).await
    }

    pub fn get_output(&self, input: &GuestInput) -> RaikoResult<GuestOutput> {
        let db = create_mem_db(&mut input.clone()).unwrap();
        let mut builder = RethBlockBuilder::new(input, db);
        let pool_tx = generate_transactions(
            &input.chain_spec,
            &input.taiko.block_proposed,
            &input.taiko.tx_data,
            &input.taiko.anchor_tx,
        );
        builder
            .execute_transactions(pool_tx, false)
            .expect("execute");

        let header = builder.finalize().map_err(|e| {
            warn!("Proving bad block construction!");
            RaikoError::Guest(raiko_lib::prover::ProverError::GuestError(e.to_string()))
        })?;

        debug!("Verifying final state using provider data ...");
        debug!(
            "Final block hash derived successfully. {}",
            header.hash_slow()
        );
        debug!("Final block header derived successfully. {header:?}");
        check_header(&input.block.header, &header)?;

        Ok(GuestOutput {
            header: header.clone(),
            hash: ProtocolInstance::new(input, &header, self.request.proof_type)?.instance_hash(),
        })
    }

    pub fn get_batch_output(&self, batch_input: &GuestBatchInput) -> RaikoResult<GuestBatchOutput> {
        info!(
            "Generating {} output for batch id: {}",
            self.request.proof_type, batch_input.taiko.batch_id
        );
        let pool_txs_list = generate_transactions_for_batch_blocks(batch_input);
        let blocks = self.build_batch_blocks(&batch_input.inputs, &pool_txs_list)?;
        verify_parent_hash_chain(&blocks)?;

        Ok(GuestBatchOutput {
            blocks: blocks.clone(),
            hash: ProtocolInstance::new_batch(batch_input, blocks, self.request.proof_type)?
                .instance_hash(),
        })
    }

    fn build_batch_blocks(
        &self,
        inputs: &[GuestInput],
        pool_txs_list: &[(Vec<reth_primitives::TransactionSigned>, bool)],
    ) -> RaikoResult<Vec<Block>> {
        inputs
            .iter()
            .zip(pool_txs_list)
            .enumerate()
            .map(|(idx, (input, (pool_txs, _)))| {
                self.single_output_for_batch(pool_txs.clone(), input, idx == 0)
            })
            .collect()
    }

    fn single_output_for_batch(
        &self,
        origin_pool_txs: Vec<reth_primitives::TransactionSigned>,
        input: &GuestInput,
        is_first_block_in_proposal: bool,
    ) -> RaikoResult<Block> {
        let db = create_mem_db(&mut input.clone()).unwrap();
        let mut builder = RethBlockBuilder::new(input, db)
            .set_is_first_block_in_proposal(is_first_block_in_proposal);

        let mut pool_txs = vec![input.taiko.anchor_tx.clone().unwrap()];
        pool_txs.extend(origin_pool_txs);

        builder
            .execute_transactions(pool_txs, false)
            .expect("execute");
        let block = builder.finalize_block().map_err(|e| {
            warn!("Proving bad block construction!");
            RaikoError::Guest(raiko_lib::prover::ProverError::GuestError(e.to_string()))
        })?;

        let header = &block.header;
        debug!(
            "Verifying final block {} state using provider data ...",
            header.number
        );
        debug!(
            "Final block {} hash derived successfully. {}",
            header.number,
            header.hash_slow()
        );
        debug!("Final block derived successfully. {block:?}");
        check_header(&input.block.header, header)?;

        Ok(block)
    }

    fn get_prover_config(&self) -> RaikoResult<Value> {
        serde_json::to_value(&self.request).map_err(Into::into)
    }

    pub async fn prove(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        store: Option<&mut dyn IdWrite>,
    ) -> RaikoResult<Proof> {
        let config = self.get_prover_config()?;
        run_prover(self.request.proof_type, input, output, &config, store).await
    }

    pub async fn batch_prove(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        store: Option<&mut dyn IdWrite>,
    ) -> RaikoResult<Proof> {
        let config = self.get_prover_config()?;
        run_batch_prover(self.request.proof_type, input, output, &config, store).await
    }

    pub async fn shasta_proposal_prove(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        store: Option<&mut dyn IdWrite>,
    ) -> RaikoResult<Proof> {
        let config = self.get_prover_config()?;
        run_shasta_proposal_prover(self.request.proof_type, input, output, &config, store).await
    }

    pub async fn cancel(
        &self,
        proof_key: ProofKey,
        read: Box<&mut dyn IdStore>,
    ) -> RaikoResult<()> {
        cancel_proof(self.request.proof_type, proof_key, read).await
    }
}

fn verify_parent_hash_chain(blocks: &[Block]) -> RaikoResult<()> {
    for window in blocks.windows(2) {
        let (parent, current) = (&window[0], &window[1]);
        if parent.header.hash_slow() != current.header.parent_hash {
            return Err(RaikoError::Guest(
                raiko_lib::prover::ProverError::GuestError("Parent hash mismatch".to_string()),
            ));
        }
    }
    Ok(())
}

fn check_header(exp: &Header, header: &Header) -> Result<(), RaikoError> {
    // Check against the expected value of all fields for easy debugability
    check_eq(&exp.parent_hash, &header.parent_hash, "parent_hash");
    check_eq(&exp.ommers_hash, &header.ommers_hash, "ommers_hash");
    check_eq(&exp.beneficiary, &header.beneficiary, "beneficiary");
    check_eq(&exp.state_root, &header.state_root, "state_root");
    check_eq(
        &exp.transactions_root,
        &header.transactions_root,
        "transactions_root",
    );
    check_eq(&exp.receipts_root, &header.receipts_root, "receipts_root");
    check_eq(
        &exp.withdrawals_root,
        &header.withdrawals_root,
        "withdrawals_root",
    );
    check_eq(&exp.logs_bloom, &header.logs_bloom, "logs_bloom");
    check_eq(&exp.difficulty, &header.difficulty, "difficulty");
    check_eq(&exp.number, &header.number, "number");
    check_eq(&exp.gas_limit, &header.gas_limit, "gas_limit");
    check_eq(&exp.gas_used, &header.gas_used, "gas_used");
    check_eq(&exp.timestamp, &header.timestamp, "timestamp");
    check_eq(&exp.mix_hash, &header.mix_hash, "mix_hash");
    check_eq(&exp.nonce, &header.nonce, "nonce");
    check_eq(
        &exp.base_fee_per_gas,
        &header.base_fee_per_gas,
        "base_fee_per_gas",
    );
    check_eq(&exp.blob_gas_used, &header.blob_gas_used, "blob_gas_used");
    check_eq(
        &exp.excess_blob_gas,
        &header.excess_blob_gas,
        "excess_blob_gas",
    );
    check_eq(
        &exp.parent_beacon_block_root,
        &header.parent_beacon_block_root,
        "parent_beacon_block_root",
    );
    check_eq(&exp.extra_data, &header.extra_data, "extra_data");

    // Block hash must match: node-provided header vs builder-derived header
    require_eq(
        &exp.hash_slow(),
        &header.hash_slow(),
        &format!("block hash unexpected for block {}", exp.number),
    )
}

fn check_eq<T: std::cmp::PartialEq + std::fmt::Debug>(expected: &T, actual: &T, message: &str) {
    let _ = black_box(require_eq(expected, actual, message));
}

fn require(expression: bool, message: &str) -> RaikoResult<()> {
    if !expression {
        let msg = format!("Assertion failed: {message}");
        error!("{msg}");
        return Err(anyhow::Error::msg(msg).into());
    }
    Ok(())
}

fn require_eq<T: std::cmp::PartialEq + std::fmt::Debug>(
    expected: &T,
    actual: &T,
    message: &str,
) -> RaikoResult<()> {
    let msg = format!("{message} - Expected: {expected:?}, Found: {actual:?}");
    require(expected == actual, &msg)
}

/// Merges two json's together, overwriting `a` with the values of `b`
pub fn merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (a, b) if !b.is_null() => b.clone_into(a),
        // If b is null, just keep a (which means do nothing).
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::interfaces::{aggregate_shasta_proposals, ShastaProposalCheckpoint};
    use crate::preflight::parse_l1_batch_proposal_tx_for_shasta_fork;
    use crate::{interfaces::ProofRequest, provider::rpc::RpcBlockDataProvider, ChainSpec, Raiko};
    use alloy_primitives::Address;
    use alloy_provider::Provider;
    use env_logger;
    use raiko_lib::input::{RawProof, ShastaAggregationGuestInput, ShastaRawAggregationGuestInput};
    use raiko_lib::protocol_instance::shasta_pcd_aggregation_hash;
    use raiko_lib::{
        consts::{Network, SupportedChainSpecs},
        input::{AggregationGuestOutput, BlobProofType, GuestBatchInput, GuestBatchOutput},
        primitives::B256,
        proof_type::ProofType,
        prover::Proof,
    };
    use reth_primitives::{address, hex};
    use serde::Serialize;
    use serde_json::{json, Value};
    use std::{collections::HashMap, env, str::FromStr};
    fn get_proof_type_from_env() -> ProofType {
        let proof_type = env::var("TARGET").unwrap_or("native".to_string());
        ProofType::from_str(&proof_type).unwrap()
    }

    fn is_ci() -> bool {
        let ci = env::var("CI").unwrap_or("0".to_string());
        ci == "1"
    }

    fn test_proof_params(enable_aggregation: bool) -> HashMap<String, Value> {
        let mut prover_args = HashMap::new();
        prover_args.insert(
            "native".to_string(),
            json! {
                {
                    "json_guest_input": null
                }
            },
        );
        prover_args.insert(
            "sp1".to_string(),
            json! {
                {
                    "recursion": if enable_aggregation { "compressed" } else { "plonk" },
                    "prover": "mock",
                    "verify": true
                }
            },
        );
        prover_args.insert(
            "risc0".to_string(),
            json! {
                {
                    "bonsai": false,
                    "snark": false,
                    "profile": true,
                    "execution_po2": 18
                }
            },
        );
        prover_args.insert(
            "sgx".to_string(),
            json! {
                {
                    "instance_id": 121,
                    "setup": enable_aggregation,
                    "bootstrap": enable_aggregation,
                    "prove": true,
                }
            },
        );
        prover_args
    }

    fn dump_file<T: Serialize>(filename: &str, data: &T) {
        let dump_mode = env::var("DUMP_FILE").unwrap_or_else(|_| "0".to_string());
        match dump_mode.as_str() {
            "1" => {
                let realname = format!("{filename}.json");
                let writer = std::fs::File::create(&realname).expect("Unable to create file");
                serde_json::to_writer(writer, data).expect("Unable to write data");
            }
            "2" => {
                let realname = format!("{filename}.bin");
                let mut writer = std::fs::File::create(realname).expect("Unable to create file");
                bincode::serialize_into(&mut writer, data).expect("Unable to write bincode data");
            }
            _ => {}
        }
    }

    async fn batch_prove_shasta_block(
        l1_chain_spec: &ChainSpec,
        taiko_chain_spec: &ChainSpec,
        proof_request: &ProofRequest,
    ) -> Proof {
        let (_block_numbers, _cached_data) = parse_l1_batch_proposal_tx_for_shasta_fork(
            l1_chain_spec,
            taiko_chain_spec,
            proof_request.l1_inclusion_block_number,
            proof_request.batch_id,
        )
        .await
        .expect("Could not parse L1 shasta proposal tx");
        let all_prove_blocks = proof_request.clone().l2_block_numbers;
        // provider target blocks are all blocks in the batch and the parent block of block[0]
        let provider_target_blocks =
            (all_prove_blocks[0] - 1..=*all_prove_blocks.last().unwrap()).collect();
        let provider =
            RpcBlockDataProvider::new_batch(&taiko_chain_spec.rpc, provider_target_blocks)
                .await
                .expect("Could not create RpcBlockDataProvider");
        let mut updated_proof_request = proof_request.clone();
        updated_proof_request.l2_block_numbers = all_prove_blocks.clone();
        let raiko = Raiko::new(
            l1_chain_spec.clone(),
            taiko_chain_spec.clone(),
            updated_proof_request.clone(),
        );
        let input = raiko
            .generate_batch_input(provider)
            .await
            .expect("input generation failed");

        dump_file(&format!("input-{}", proof_request.batch_id), &input);

        let output = raiko
            .get_batch_output(&input)
            .expect("output generation failed");

        dump_file(&format!("output-{}", proof_request.batch_id), &output);
        raiko
            .shasta_proposal_prove(input, &output, None)
            .await
            .expect("proof generation failed")
    }

    async fn aggregate_single_shasta_proof(proof_request: &ProofRequest, proof: &Proof) -> Proof {
        let proof_type = proof_request.proof_type;
        let input = ShastaAggregationGuestInput {
            proofs: vec![proof.clone()],
        };
        let guest_input = ShastaRawAggregationGuestInput {
            proofs: vec![RawProof {
                input: proof.input.clone().unwrap(),
                proof: {
                    if proof.proof.is_some() {
                        hex::decode(&proof.proof.clone().unwrap()[2..])
                            .expect("invalid hex data in proof.proof")
                    } else {
                        Default::default()
                    }
                },
            }],
            proof_carry_data_vec: vec![proof.extra_data.clone().unwrap()],
        };
        dump_file(
            &format!("agg-input-{}", proof_request.batch_id),
            &guest_input,
        );

        let aggregate_hash =
            shasta_pcd_aggregation_hash(&guest_input.proof_carry_data_vec, Address::ZERO)
                .expect("failed to get aggregate hash");
        let output = AggregationGuestOutput {
            hash: aggregate_hash,
        };

        dump_file(&format!("agg-output-{}", proof_request.batch_id), &output);
        let config = Value::default();
        aggregate_shasta_proposals(proof_type, input, &output, &config, None)
            .await
            .expect("failed to generate aggregation proof")
    }

    #[ignore]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_shasta_proposal_block_taiko_dev() {
        env_logger::init();
        let proof_type = get_proof_type_from_env();
        let l1_network = "taiko_dev_l1".to_owned();
        let network = "taiko_dev".to_owned();
        let chain_specs = SupportedChainSpecs::merge_from_file(
            "../host/config/chain_spec_list_devnet.json".into(),
        )
        .unwrap();
        let taiko_chain_spec = chain_specs.get_chain_spec(&network).unwrap();
        let l1_chain_spec = chain_specs.get_chain_spec(&l1_network).unwrap();
        let proof_request = ProofRequest {
            block_number: 0,
            batch_id: 1,
            l1_inclusion_block_number: 2174455,
            l2_block_numbers: vec![3951006],
            network,
            graffiti: B256::ZERO,
            prover: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
            l1_network,
            proof_type,
            blob_proof_type: BlobProofType::ProofOfEquivalence,
            prover_args: test_proof_params(false),
            checkpoint: None,
            cached_event_data: None,
            last_anchor_block_number: Some(0),
        };

        let proof =
            batch_prove_shasta_block(&l1_chain_spec, &taiko_chain_spec, &proof_request).await;
        let aggregated_proof = aggregate_single_shasta_proof(&proof_request, &proof).await;
        println!("aggregated shasta proof: {aggregated_proof:?}");
    }
}
