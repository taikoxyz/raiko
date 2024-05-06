use alloy_primitives::{Address, FixedBytes, B256, U256};
use alloy_rpc_types::Block;
use anyhow::Result;
use raiko_lib::builder::{BlockBuilderStrategy, TaikoStrategy};
use raiko_lib::consts::ChainSpec;
use raiko_lib::input::{GuestInput, GuestOutput, TaikoProverData, WrappedHeader};
use raiko_lib::protocol_instance::{assemble_protocol_instance, ProtocolInstance};
use raiko_lib::prover::{to_proof, Proof, Prover, ProverError, ProverResult};
use raiko_lib::utils::HeaderHasher;
use revm::primitives::AccountInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{trace, warn};

use crate::error::{self, HostError};
use crate::preflight::preflight;
use crate::request::ProofRequest;
use crate::MerkleProof;

#[allow(async_fn_in_trait)]
pub trait BlockDataProvider {
    async fn get_blocks(
        &self,
        blocks_to_fetch: &[(u64, bool)],
    ) -> Result<Vec<Block>, anyhow::Error>;
    async fn get_accounts(&self, accounts: &[Address]) -> Result<Vec<AccountInfo>, anyhow::Error>;
    async fn get_storage_values(
        &self,
        accounts: &[(Address, U256)],
    ) -> Result<Vec<U256>, anyhow::Error>;
    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> Result<MerkleProof, anyhow::Error>;
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
    ) -> Result<GuestInput, HostError> {
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

    pub fn get_output(&self, input: &GuestInput) -> Result<GuestOutput, HostError> {
        match TaikoStrategy::build_from(input) {
            Ok((header, _mpt_node)) => {
                println!("Verifying final state using provider data ...");
                println!("Final block hash derived successfully. {}", header.hash());
                println!("Final block header derived successfully. {:?}", header);
                let pi = NativeProver::instance_hash(assemble_protocol_instance(&input, &header)?);

                // Check against the expected value of all fields for easy debugability
                let exp = &input.block_header_reference;
                check_eq(exp.parent_hash, header.parent_hash, "base_fee_per_gas");
                check_eq(exp.ommers_hash, header.ommers_hash, "ommers_hash");
                check_eq(exp.beneficiary, header.beneficiary, "beneficiary");
                check_eq(exp.state_root, header.state_root, "state_root");
                check_eq(
                    exp.transactions_root,
                    header.transactions_root,
                    "transactions_root",
                );
                check_eq(exp.receipts_root, header.receipts_root, "receipts_root");
                check_eq(
                    exp.withdrawals_root,
                    header.withdrawals_root,
                    "withdrawals_root",
                );
                check_eq(exp.logs_bloom, header.logs_bloom, "logs_bloom");
                check_eq(exp.difficulty, header.difficulty, "difficulty");
                check_eq(exp.number, header.number, "number");
                check_eq(exp.gas_limit, header.gas_limit, "gas_limit");
                check_eq(exp.gas_used, header.gas_used, "gas_used");
                check_eq(exp.timestamp, header.timestamp, "timestamp");
                check_eq(exp.mix_hash, header.mix_hash, "mix_hash");
                check_eq(exp.nonce, header.nonce, "nonce");
                check_eq(
                    exp.base_fee_per_gas,
                    header.base_fee_per_gas,
                    "base_fee_per_gas",
                );
                check_eq(exp.blob_gas_used, header.blob_gas_used, "blob_gas_used");
                check_eq(
                    exp.excess_blob_gas,
                    header.excess_blob_gas,
                    "excess_blob_gas",
                );
                check_eq(
                    exp.parent_beacon_block_root,
                    header.parent_beacon_block_root,
                    "parent_beacon_block_root",
                );
                check_eq(
                    exp.extra_data.clone(),
                    header.extra_data.clone(),
                    "extra_data",
                );

                // Make sure the blockhash from the node matches the one from the builder
                assert_eq!(
                    Into::<FixedBytes<32>>::into(header.hash().0),
                    input.block_hash_reference,
                    "block hash unexpected"
                );
                let output = GuestOutput::Success((
                    WrappedHeader {
                        header: header.clone(),
                    },
                    pi,
                ));

                Ok(output)
            }
            Err(e) => {
                warn!("Proving bad block construction!");
                Err(HostError::GuestError(
                    raiko_lib::prover::ProverError::GuestError(e.to_string()),
                ))
            }
        }
    }

    pub async fn prove(
        &self,
        input: GuestInput,
        output: &GuestOutput,
    ) -> Result<serde_json::Value, HostError> {
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
        trace!("Running the native prover for input {:?}", input);
        match output.clone() {
            GuestOutput::Success((wrapped_header, _)) => {
                assemble_protocol_instance(&input, &wrapped_header.header)
                    .map_err(|e| ProverError::GuestError(e.to_string()))?;
            }
            _ => return Err(ProverError::GuestError("Unexpected output".to_string())),
        }

        to_proof(Ok(NativeResponse {
            output: output.clone(),
        }))
    }

    fn instance_hash(_pi: ProtocolInstance) -> B256 {
        B256::default()
    }
}

fn check_eq<T: std::cmp::PartialEq + std::fmt::Debug>(expected: T, actual: T, message: &str) {
    if expected != actual {
        println!(
            "Assertion failed: {} - Expected: {:?}, Found: {:?}",
            message, expected, actual
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::raiko::{ChainSpec, NativeResponse, Raiko};
    use crate::request::{ProofRequest, ProofType};
    use crate::rpc_provider::RpcBlockDataProvider;
    use alloy_primitives::Address;
    use raiko_lib::{
        consts::{get_network_spec, Network},
        input::GuestOutput,
    };
    use raiko_primitives::B256;
    use std::collections::HashMap;

    async fn prove_block(chain_spec: ChainSpec, proof_request: ProofRequest) {
        let provider =
            RpcBlockDataProvider::new(&proof_request.rpc.clone(), proof_request.block_number - 1);
        let raiko = Raiko::new(chain_spec, proof_request);
        let input = raiko
            .generate_input(provider)
            .await
            .expect("input generation failed");
        let output = raiko.get_output(&input).expect("output generation failed");
        let proof = raiko
            .prove(input, &output)
            .await
            .expect("proof generation failed");
        let response: NativeResponse = serde_json::from_value(proof).unwrap();
        match response.output {
            GuestOutput::Success(_) => {}
            GuestOutput::Failure => unreachable!(),
        };
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_taiko_a7() {
        let network = Network::TaikoA7;
        let block_number = 39367;
        let chain_spec = get_network_spec(network);
        let proof_request = ProofRequest {
            block_number,
            rpc: "https://rpc.hekla.taiko.xyz/".to_string(),
            l1_rpc: "https://l1rpc.hekla.taiko.xyz/".to_string(),
            beacon_rpc: "https://l1beacon.hekla.taiko.xyz".to_string(),
            network,
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            l1_network: Network::Ethereum.to_string(),
            proof_type: ProofType::Native,
            prover_args: HashMap::new(),
        };
        prove_block(chain_spec, proof_request).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_ethereum() {
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
            proof_type: ProofType::Native,
            prover_args: HashMap::new(),
        };
        prove_block(chain_spec, proof_request).await;
    }
}
