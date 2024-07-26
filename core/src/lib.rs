use crate::{
    interfaces::{ProofRequest, RaikoError, RaikoResult},
    preflight::preflight,
    provider::BlockDataProvider,
};
use alloy_primitives::Address;
use alloy_rpc_types::EIP1186AccountProofResponse;
use raiko_lib::protocol_instance::ProtocolInstance;
use raiko_lib::prover::Proof;
use raiko_lib::{
    builder::{create_mem_db, RethBlockBuilder},
    prover::ProofKey,
};
use raiko_lib::{
    consts::ChainSpec,
    input::{GuestInput, GuestOutput, TaikoProverData},
    prover::{IdStore, IdWrite},
};
use reth_primitives::Header;
use serde_json::Value;
use std::{collections::HashMap, hint::black_box};
use tracing::{debug, error, info, warn};

pub mod interfaces;
pub mod preflight;
pub mod prover;
pub mod provider;

pub type MerkleProof = HashMap<Address, EIP1186AccountProofResponse>;

pub struct Raiko {
    l1_chain_spec: ChainSpec,
    taiko_chain_spec: ChainSpec,
    request: ProofRequest,
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

    pub async fn generate_input<BDP: BlockDataProvider>(
        &self,
        provider: BDP,
    ) -> RaikoResult<GuestInput> {
        //TODO: read fork from config
        if self.request.block_number <= 999999999 {
            preflight(
                provider,
                self.request.block_number,
                self.l1_chain_spec.to_owned(),
                self.taiko_chain_spec.to_owned(),
                TaikoProverData {
                    graffiti: self.request.graffiti,
                    prover: self.request.prover,
                },
                self.request.blob_proof_type.clone(),
            )
            .await
            .map_err(Into::<RaikoError>::into)
        } else {
            crate::preflight::ontake::preflight(
                provider,
                self.request.block_number,
                self.request.block_number,
                self.l1_chain_spec.to_owned(),
                self.taiko_chain_spec.to_owned(),
                TaikoProverData {
                    graffiti: self.request.graffiti,
                    prover: self.request.prover,
                },
                self.request.blob_proof_type.clone(),
            )
            .await
            .map_err(Into::<RaikoError>::into)
        }
    }

    pub fn get_output(&self, input: &GuestInput) -> RaikoResult<GuestOutput> {
        let db = create_mem_db(&mut input.clone()).unwrap();
        let mut builder = RethBlockBuilder::new(input, db);
        builder.execute_transactions(false).expect("execute");
        let result = builder.finalize();

        match result {
            Ok(header) => {
                info!("Verifying final state using provider data ...");
                info!(
                    "Final block hash derived successfully. {}",
                    header.hash_slow()
                );
                debug!("Final block header derived successfully. {header:?}");
                // Check if the header is the expected one
                check_header(&input.block.header, &header)?;

                Ok(GuestOutput {
                    header: header.clone(),
                    hash: ProtocolInstance::new(input, &header, self.request.proof_type.into())?
                        .instance_hash(),
                })
            }
            Err(e) => {
                warn!("Proving bad block construction!");
                Err(RaikoError::Guest(
                    raiko_lib::prover::ProverError::GuestError(e.to_string()),
                ))
            }
        }
    }

    pub async fn prove(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        store: Option<&mut dyn IdWrite>,
    ) -> RaikoResult<Proof> {
        let config = serde_json::to_value(&self.request)?;
        self.request
            .proof_type
            .run_prover(input, output, &config, store)
            .await
    }

    pub async fn cancel(
        &self,
        proof_key: ProofKey,
        read: Box<&mut dyn IdStore>,
    ) -> RaikoResult<()> {
        self.request.proof_type.cancel_proof(proof_key, read).await
    }
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

    // Make sure the blockhash from the node matches the one from the builder
    require_eq(
        &exp.hash_slow(),
        &header.hash_slow(),
        &format!("block hash unexpected for block {}", exp.number),
    )
}

fn check_eq<T: std::cmp::PartialEq + std::fmt::Debug>(expected: &T, actual: &T, message: &str) {
    // printing out error, if any, but ignoring the result
    // making sure it's not optimized out
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
    use crate::{
        interfaces::{ProofRequest, ProofType},
        provider::rpc::RpcBlockDataProvider,
        ChainSpec, Raiko,
    };
    use alloy_primitives::Address;
    use alloy_provider::Provider;
    use clap::ValueEnum;
    use raiko_lib::{
        consts::{Network, SupportedChainSpecs},
        input::BlobProofType,
        primitives::B256,
    };
    use serde_json::{json, Value};
    use std::{collections::HashMap, env};

    fn get_proof_type_from_env() -> ProofType {
        let proof_type = env::var("TARGET").unwrap_or("native".to_string());
        ProofType::from_str(&proof_type, true).unwrap()
    }

    fn is_ci() -> bool {
        let ci = env::var("CI").unwrap_or("0".to_string());
        ci == "1"
    }

    fn test_proof_params() -> HashMap<String, Value> {
        let mut prover_args = HashMap::new();
        prover_args.insert(
            "native".to_string(),
            json! {
                {
                    "write_guest_input_path": null
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
                    "setup": true,
                    "bootstrap": true,
                    "prove": true,
                }
            },
        );
        prover_args
    }

    async fn prove_block(
        l1_chain_spec: ChainSpec,
        taiko_chain_spec: ChainSpec,
        proof_request: ProofRequest,
    ) {
        let provider =
            RpcBlockDataProvider::new(&taiko_chain_spec.rpc, proof_request.block_number - 1)
                .expect("Could not create RpcBlockDataProvider");
        let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request.clone());
        let input = raiko
            .generate_input(provider)
            .await
            .expect("input generation failed");
        let output = raiko.get_output(&input).expect("output generation failed");
        let _proof = raiko
            .prove(input, &output, None)
            .await
            .expect("proof generation failed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_taiko_a7() {
        let proof_type = get_proof_type_from_env();
        let l1_network = Network::Holesky.to_string();
        let network = Network::TaikoA7.to_string();
        // Give the CI an simpler block to test because it doesn't have enough memory.
        // Unfortunately that also means that kzg is not getting fully verified by CI.
        let block_number = if is_ci() { 105987 } else { 101368 };
        let taiko_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&network)
            .unwrap();
        let l1_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&l1_network)
            .unwrap();

        let proof_request = ProofRequest {
            block_number,
            network,
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            l1_network,
            proof_type,
            blob_proof_type: BlobProofType::ProofOfEquivalence,
            prover_args: test_proof_params(),
        };
        prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
    }

    async fn get_recent_block_num(chain_spec: &ChainSpec) -> u64 {
        let provider = RpcBlockDataProvider::new(&chain_spec.rpc, 0).unwrap();
        let height = provider.provider.get_block_number().await.unwrap();
        height - 100
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_ethereum() {
        let proof_type = get_proof_type_from_env();
        // Skip test on SP1 for now because it's too slow on CI
        if !(is_ci() && proof_type == ProofType::Sp1) {
            let network = Network::Ethereum.to_string();
            let l1_network = Network::Ethereum.to_string();
            let taiko_chain_spec = SupportedChainSpecs::default()
                .get_chain_spec(&network)
                .unwrap();
            let l1_chain_spec = SupportedChainSpecs::default()
                .get_chain_spec(&l1_network)
                .unwrap();
            let block_number = get_recent_block_num(&taiko_chain_spec).await;
            println!(
                "test_prove_block_ethereum in block_number: {}",
                block_number
            );
            let proof_request = ProofRequest {
                block_number,
                network,
                graffiti: B256::ZERO,
                prover: Address::ZERO,
                l1_network,
                proof_type,
                blob_proof_type: BlobProofType::ProofOfEquivalence,
                prover_args: test_proof_params(),
            };
            prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_taiko_mainnet() {
        let proof_type = get_proof_type_from_env();
        // Skip test on SP1 for now because it's too slow on CI
        if !(is_ci() && proof_type == ProofType::Sp1) {
            let network = Network::TaikoMainnet.to_string();
            let l1_network = Network::Ethereum.to_string();
            let taiko_chain_spec = SupportedChainSpecs::default()
                .get_chain_spec(&network)
                .unwrap();
            let l1_chain_spec = SupportedChainSpecs::default()
                .get_chain_spec(&l1_network)
                .unwrap();
            let block_number = get_recent_block_num(&taiko_chain_spec).await;
            println!(
                "test_prove_block_taiko_mainnet in block_number: {}",
                block_number
            );
            let proof_request = ProofRequest {
                block_number,
                network,
                graffiti: B256::ZERO,
                prover: Address::ZERO,
                l1_network,
                proof_type,
                blob_proof_type: BlobProofType::ProofOfEquivalence,
                prover_args: test_proof_params(),
            };
            prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
        }
    }
}
