use alloy_primitives::{Address, FixedBytes, B256, U256};
use alloy_rpc_types::Block;
use raiko_lib::builder::{BlockBuilderStrategy, TaikoStrategy};
use raiko_lib::consts::ChainSpec;
use raiko_lib::input::{GuestInput, GuestOutput, TaikoProverData};
use raiko_lib::protocol_instance::{assemble_protocol_instance, ProtocolInstance};
use raiko_lib::prover::{to_proof, Proof, Prover, ProverError, ProverResult};
use raiko_lib::utils::HeaderHasher;
use revm::primitives::AccountInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{error, info, trace, warn};

use crate::error::{self, HostError, HostResult};
use crate::preflight::preflight;
use crate::request::ProofRequest;
use crate::MerkleProof;

#[allow(async_fn_in_trait)]
pub trait BlockDataProvider {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> HostResult<Vec<Block>>;

    async fn get_accounts(&self, accounts: &[Address]) -> HostResult<Vec<AccountInfo>>;

    async fn get_storage_values(&self, accounts: &[(Address, U256)]) -> HostResult<Vec<U256>>;

    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> HostResult<MerkleProof>;
}

pub struct Raiko {
    chain_spec: ChainSpec,
    request: ProofRequest,
}

impl Raiko {
    pub fn new(chain_spec: ChainSpec, request: ProofRequest) -> Self {
        Self {
            chain_spec,
            request,
        }
    }

    pub async fn generate_input<BDP: BlockDataProvider>(
        &self,
        provider: BDP,
    ) -> HostResult<GuestInput> {
        preflight(
            provider,
            self.request.block_number,
            self.chain_spec.clone(),
            TaikoProverData {
                graffiti: self.request.graffiti,
                prover: self.request.prover,
            },
            Some(self.request.l1_rpc.clone()),
            Some(self.request.beacon_rpc.clone()),
        )
        .await
        .map_err(Into::<error::HostError>::into)
    }

    pub fn get_output(&self, input: &GuestInput) -> HostResult<GuestOutput> {
        match TaikoStrategy::build_from(input) {
            Ok((header, _mpt_node)) => {
                info!("Verifying final state using provider data ...");
                info!("Final block hash derived successfully. {}", header.hash());
                info!("Final block header derived successfully. {header:?}");
                let pi = self
                    .request
                    .proof_type
                    .instance_hash(assemble_protocol_instance(input, &header)?)?;

                // Check against the expected value of all fields for easy debugability
                let exp = &input.block_header_reference;
                check_eq(&exp.parent_hash, &header.parent_hash, "base_fee_per_gas");
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
                check_eq(
                    &exp.extra_data.clone(),
                    &header.extra_data.clone(),
                    "extra_data",
                );

                // Make sure the blockhash from the node matches the one from the builder
                assert_eq!(
                    Into::<FixedBytes<32>>::into(header.hash().0),
                    input.block_hash_reference,
                    "block hash unexpected"
                );
                let output = GuestOutput::Success { header, hash: pi };

                Ok(output)
            }
            Err(e) => {
                warn!("Proving bad block construction!");
                Err(HostError::Guest(
                    raiko_lib::prover::ProverError::GuestError(e.to_string()),
                ))
            }
        }
    }

    pub async fn prove(
        &self,
        input: GuestInput,
        output: &GuestOutput,
    ) -> HostResult<serde_json::Value> {
        self.request
            .proof_type
            .run_prover(
                input.clone(),
                output,
                &serde_json::to_value(self.request.clone())?,
            )
            .await
    }
}

pub struct NativeProver;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeResponse {
    pub output: GuestOutput,
}

impl Prover for NativeProver {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        _request: &serde_json::Value,
    ) -> ProverResult<Proof> {
        trace!("Running the native prover for input {input:?}");

        let GuestOutput::Success { header, .. } = output.clone() else {
            return Err(ProverError::GuestError("Unexpected output".to_owned()));
        };

        assemble_protocol_instance(&input, &header)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;

        to_proof(Ok(NativeResponse {
            output: output.clone(),
        }))
    }

    fn instance_hash(_pi: ProtocolInstance) -> B256 {
        B256::default()
    }
}

fn check_eq<T: std::cmp::PartialEq + std::fmt::Debug>(expected: &T, actual: &T, message: &str) {
    if expected != actual {
        error!("Assertion failed: {message} - Expected: {expected:?}, Found: {actual:?}");
    }
}

#[cfg(test)]
mod tests {
    use crate::raiko::{ChainSpec, Raiko};
    use crate::request::{ProofRequest, ProofType};
    use crate::rpc_provider::RpcBlockDataProvider;
    use alloy_primitives::Address;
    use clap::ValueEnum;
    use raiko_lib::consts::{get_network_spec, Network};
    use raiko_primitives::B256;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::env;

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

    async fn prove_block(chain_spec: ChainSpec, proof_request: ProofRequest) {
        let provider =
            RpcBlockDataProvider::new(&proof_request.rpc.clone(), proof_request.block_number - 1)
                .expect("Could not create RpcBlockDataProvider");
        let raiko = Raiko::new(chain_spec, proof_request.clone());
        let mut input = raiko
            .generate_input(provider)
            .await
            .expect("input generation failed");
        if is_ci() && proof_request.proof_type == ProofType::Sp1 {
            input.taiko.skip_verify_blob = true;
        }
        let output = raiko.get_output(&input).expect("output generation failed");
        let _proof = raiko
            .prove(input, &output)
            .await
            .expect("proof generation failed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_taiko_a7() {
        let proof_type = get_proof_type_from_env();
        let network = Network::TaikoA7;
        let block_number = 105987;
        let chain_spec = get_network_spec(network);
        let proof_request = ProofRequest {
            block_number,
            rpc: "https://rpc.hekla.taiko.xyz".to_string(),
            l1_rpc: "https://ethereum-holesky-rpc.publicnode.com".to_string(),
            beacon_rpc: "https://eth-holesky-beacon.public.blastapi.io".to_string(),
            network,
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            l1_network: Network::Ethereum.to_string(),
            proof_type,
            prover_args: test_proof_params(),
        };
        prove_block(chain_spec, proof_request).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_ethereum() {
        let proof_type = get_proof_type_from_env();
        // Skip test on SP1 for now because it's too slow on CI
        if proof_type != ProofType::Sp1 {
            let network = Network::Ethereum;
            let block_number = 19707175;
            let chain_spec = get_network_spec(network);
            let proof_request = ProofRequest {
                block_number,
                rpc: "https://rpc.ankr.com/eth".to_string(),
                l1_rpc: String::new(),
                beacon_rpc: String::new(),
                network,
                graffiti: B256::ZERO,
                prover: Address::ZERO,
                l1_network: Network::Ethereum.to_string(),
                proof_type,
                prover_args: test_proof_params(),
            };
            prove_block(chain_spec, proof_request).await;
        }
    }
}
