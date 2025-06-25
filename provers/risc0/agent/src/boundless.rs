use std::time::Duration;

use crate::methods::{
    boundless_aggregation::BOUNDLESS_AGGREGATION_ELF,
    boundless_batch::{BOUNDLESS_BATCH_ELF, BOUNDLESS_BATCH_ID},
};
use alloy_primitives::B256;
use alloy_primitives_v1p2p0::{U256, utils::parse_ether};
use alloy_signer_local_v1p0p12::PrivateKeySigner;
use boundless_market::{
    Client, deployments::SEPOLIA, input::GuestEnv, request_builder::OfferParams,
};
use reqwest::Url;
use risc0_ethereum_contracts_boundless::receipt::Receipt as ContractReceipt;
use risc0_zkvm::{Digest, Receipt as ZkvmReceipt, default_executor};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundlessAggregationGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<ZkvmReceipt>,
}

use tokio::sync::OnceCell;

static RISCV_PROVER: OnceCell<Risc0BoundlessProver> = OnceCell::const_new();

pub async fn get_boundless_prover() -> &'static Risc0BoundlessProver {
    RISCV_PROVER
        .get_or_init(|| async {
            Risc0BoundlessProver::init_prover()
                .await
                .expect("Failed to initialize Boundless client")
        })
        .await
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Risc0Response {
    pub seal: Vec<u8>,
    pub journal: Vec<u8>,
    pub receipt: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Risc0BoundlessProver {
    batch_image_url: Option<Url>,
    aggregation_image_url: Option<Url>,
}

impl Risc0BoundlessProver {
    pub async fn get() -> &'static Self {
        get_boundless_prover().await
    }

    pub async fn init_prover() -> AgentResult<Self> {
        let deployment = Some(SEPOLIA);
        let storage_provider = boundless_market::storage::storage_provider_from_env().ok();
        let boundless_client = {
            // Create a Boundless client from the provided parameters.
            // let args = helper::Args::parse();
            let url = Url::parse("https://ethereum-sepolia-rpc.publicnode.com").unwrap();
            // let order_stream_url = Url::parse("https://eth-sepolia.beboundless.xyz").ok();
            let sender_priv_key = std::env::var("BOUNDLESS_SIGNER_KEY").unwrap_or_else(|_| {
                panic!("BOUNDLESS_SIGNER_KEY is not set");
            });
            let signer: PrivateKeySigner = sender_priv_key.parse().unwrap();

            // Create a Boundless client from the provided parameters.
            let client = Client::builder()
                .with_rpc_url(url)
                .with_deployment(deployment)
                .with_storage_provider(storage_provider)
                .with_private_key(signer)
                .build()
                .await
                .map_err(|e| {
                    AgentError::AgentError(format!("Failed to build boundless client: {e}"))
                })?;
            client
        };

        // Upload the ELF to the storage provider so that it can be fetched by the market.
        assert!(
            boundless_client.storage_provider.is_some(),
            "a storage provider is required to upload the zkVM guest ELF"
        );

        let batch_image_url = boundless_client
            .upload_program(BOUNDLESS_BATCH_ELF)
            .await
            .map_err(|e| {
                AgentError::AgentError(format!("Failed to upload BOUNDLESS_BATCH_ELF image: {e}"))
            })?;

        let aggregation_image_url = boundless_client
            .upload_program(BOUNDLESS_AGGREGATION_ELF)
            .await
            .map_err(|e| {
                AgentError::AgentError(format!(
                    "Failed to upload BOUNDLESS_AGGREGATION_ELF image: {e}"
                ))
            })?;

        Ok(Risc0BoundlessProver {
            batch_image_url: Some(batch_image_url),
            aggregation_image_url: Some(aggregation_image_url),
        })
    }

    pub async fn get_batch_image_url(&self) -> Option<Url> {
        self.batch_image_url.clone()
    }

    pub async fn get_aggregation_image_url(&self) -> Option<Url> {
        self.aggregation_image_url.clone()
    }
}

// Simplified error type
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Agent error: {0}")]
    AgentError(String),
}

pub type AgentResult<T> = Result<T, AgentError>;

impl Risc0BoundlessProver {
    pub async fn run(
        &self,
        _input: Vec<u8>,
        _output: &[u8],
        _config: &serde_json::Value,
    ) -> AgentResult<Vec<u8>> {
        unimplemented!("No need for post pacaya");
    }

    pub async fn aggregate(
        &self,
        _input: Vec<u8>,
        _output: &[u8],
        _config: &serde_json::Value,
    ) -> AgentResult<Vec<u8>> {
        let encoded_input = _input;
        let guest_env = GuestEnv::builder().write_frame(&encoded_input).build_env();
        let guest_env_bytes = guest_env.clone().encode().map_err(|e| {
            AgentError::AgentError(format!("Failed to encode guest environment: {e}"))
        })?;

        tracing::info!(
            "len guest_env_bytes (aggregate): {:?}",
            guest_env_bytes.len()
        );
        let (mcycles_count, _journal) = {
            // Dry run the ELF with the input to get the journal and cycle count.
            let session_info = default_executor()
                .execute(
                    guest_env.clone().try_into().unwrap(),
                    BOUNDLESS_AGGREGATION_ELF,
                )
                .map_err(|e| {
                    AgentError::AgentError(format!("Failed to execute guest environment: {e}"))
                })?;
            let mcycles_count = session_info
                .segments
                .iter()
                .map(|segment| 1 << segment.po2)
                .sum::<u64>()
                .div_ceil(1_000_000);
            let journal = session_info.journal;
            (mcycles_count, journal)
        };
        tracing::info!("mcycles_count (aggregate): {}", mcycles_count);

        let boundless_client = {
            let deployment = Some(SEPOLIA);
            let storage_provider = boundless_market::storage::storage_provider_from_env().ok();

            // Create a Boundless client from the provided parameters.
            // let args = helper::Args::parse();
            let url = Url::parse("https://ethereum-sepolia-rpc.publicnode.com").unwrap();
            // let order_stream_url = Url::parse("https://eth-sepolia.beboundless.xyz").ok();
            let sender_priv_key = std::env::var("BOUNDLESS_SIGNER_KEY").unwrap_or_else(|_| {
                panic!("BOUNDLESS_SIGNER_KEY is not set");
            });
            let signer: PrivateKeySigner = sender_priv_key.parse().unwrap();

            // Create a Boundless client from the provided parameters.
            let client = Client::builder()
                .with_rpc_url(url)
                .with_deployment(deployment)
                .with_storage_provider(storage_provider)
                .with_private_key(signer)
                .build()
                .await
                .map_err(|e| {
                    AgentError::AgentError(format!("Failed to build boundless client: {e}"))
                })?;
            client
        };

        // Upload the input to the storage provider
        let input_url = boundless_client
            .upload_input(&guest_env_bytes)
            .await
            .map_err(|e| AgentError::AgentError(format!("Failed to upload input: {e}")))?;

        // Prepare the order for aggregation
        let aggregation_image_url = self
            .aggregation_image_url
            .clone()
            .ok_or_else(|| AgentError::AgentError("Aggregation image URL not set".to_string()))?;

        // add 1 minute for each 1M cycles to the original timeout
        // Use the input directly as the estimated cycle count, since we are using a loop program.
        let m_cycles = mcycles_count;
        let ramp_up = 1000;
        let lock_timeout = 2000;
        // Give equal time for provers that are fulfilling after lock expiry to prove.
        let timeout: u32 = 4000;

        let request = boundless_client
            .new_request()
            .with_program_url(aggregation_image_url)
            .unwrap()
            .with_groth16_proof()
            .with_input_url(input_url)
            .unwrap()
            .with_offer(
                OfferParams::builder()
                    .ramp_up_period(ramp_up)
                    .lock_timeout(lock_timeout)
                    .timeout(timeout)
                    .max_price(parse_ether("0.0005").unwrap_or_default() * U256::from(m_cycles))
                    .min_price(parse_ether("0.0001").unwrap_or_default() * U256::from(m_cycles))
                    .lock_stake(U256::from(m_cycles * 100)),
            );

        // Build the request, including preflight, and assigned the remaining fields.
        let request = boundless_client
            .build_request(request)
            .await
            .map_err(|e| AgentError::AgentError(format!("Failed to build request: {e}")))?;
        tracing::info!("Request: {:?}", request);

        let offchain = false;
        // Send the request and wait for it to be completed.
        let (request_id, expires_at) = if offchain {
            unimplemented!("offchain is not supported");
        } else {
            boundless_client
                .submit_request_onchain(&request)
                .await
                .map_err(|e| {
                    AgentError::AgentError(format!("Failed to submit request onchain: {e}"))
                })?
        };
        tracing::info!("Request 0x{request_id:x} submitted");

        // Wait for the request to be fulfilled by the market, returning the journal and seal.
        tracing::info!("Waiting for 0x{request_id:x} to be fulfilled");
        let (journal, seal) = boundless_client
            .wait_for_request_fulfillment(request_id, Duration::from_secs(10), expires_at)
            .await
            .map_err(|e| {
                AgentError::AgentError(format!("Failed to wait for request fulfillment: {e}"))
            })?;
        tracing::info!(
            "Request 0x{request_id:x} fulfilled. Journal: {:?}, Seal: {:?}, image_id: {:?}",
            journal,
            seal,
            BOUNDLESS_BATCH_ID,
        );

        let response = Risc0Response {
            seal: seal.to_vec(),
            journal: journal.to_vec(),
            receipt: None,
        };
        // Use bincode to serialize the response and return as Vec<u8>
        let proof_bytes = bincode::serialize(&response)
            .map_err(|e| AgentError::AgentError(format!("Failed to encode response: {e}")))?;
        return Ok(proof_bytes);
    }

    pub async fn cancel(&self, _key: (u64, u64, B256, u8)) -> AgentResult<()> {
        todo!()
    }

    pub async fn batch_run(
        &self,
        _input: Vec<u8>,
        _output: &[u8],
        _config: &serde_json::Value,
    ) -> AgentResult<Vec<u8>> {
        // Encode the input and upload it to the storage provider.
        let encoded_input = _input;
        let guest_env = GuestEnv::builder().write_frame(&encoded_input).build_env();
        let guest_env_bytes = guest_env.clone().encode().map_err(|e| {
            AgentError::AgentError(format!("Failed to encode guest environment: {e}"))
        })?;

        tracing::info!("len guest_env_bytes: {:?}", guest_env_bytes.len());
        let (mcycles_count, _journal) = {
            // Dry run the ELF with the input to get the journal and cycle count.
            // This can be useful to estimate the cost of the proving request.
            // It can also be useful to ensure the guest can be executed correctly and we do not send into
            // the market unprovable proving requests. If you have a different mechanism to get the expected
            // journal and set a price, you can skip this step.
            let session_info = default_executor()
                .execute(guest_env.clone().try_into().unwrap(), BOUNDLESS_BATCH_ELF)
                .map_err(|e| {
                    AgentError::AgentError(format!("Failed to execute guest environment: {e}"))
                })?;
            let mcycles_count = session_info
                .segments
                .iter()
                .map(|segment| 1 << segment.po2)
                .sum::<u64>()
                .div_ceil(1_000_000);
            let journal = session_info.journal;
            (mcycles_count, journal)
        };
        tracing::info!("mcycles_count: {}", mcycles_count);

        let boundless_client = {
            let deployment = Some(SEPOLIA);
            let storage_provider = boundless_market::storage::storage_provider_from_env().ok();

            // Create a Boundless client from the provided parameters.
            // let args = helper::Args::parse();
            let url = Url::parse("https://ethereum-sepolia-rpc.publicnode.com").unwrap();
            // let order_stream_url = Url::parse("https://eth-sepolia.beboundless.xyz").ok();
            let sender_priv_key = std::env::var("BOUNDLESS_SIGNER_KEY").unwrap_or_else(|_| {
                panic!("BOUNDLESS_SIGNER_KEY is not set");
            });
            let signer: PrivateKeySigner = sender_priv_key.parse().unwrap();

            // Create a Boundless client from the provided parameters.
            let client = Client::builder()
                .with_rpc_url(url)
                .with_deployment(deployment)
                .with_storage_provider(storage_provider)
                .with_private_key(signer)
                .build()
                .await
                .map_err(|e| {
                    AgentError::AgentError(format!("Failed to build boundless client: {e}"))
                })?;
            client
        };

        let input_url = boundless_client
            .upload_input(&guest_env_bytes)
            .await
            .map_err(|e| AgentError::AgentError(format!("Failed to upload input: {e}")))?;
        tracing::info!("Uploaded input to {}", input_url);

        // add 1 minute for each 1M cycles to the original timeout
        // Use the input directly as the estimated cycle count, since we are using a loop program.
        let m_cycles = mcycles_count;
        let ramp_up = 1000;
        let lock_timeout = 2000;
        // Give equal time for provers that are fulfilling after lock expiry to prove.
        let timeout: u32 = 4000;

        let request = boundless_client
            .new_request()
            .with_program_url(self.batch_image_url.clone().unwrap())
            .unwrap()
            .with_groth16_proof()
            // .with_env(guest_env)
            .with_input_url(input_url)
            .unwrap()
            .with_offer(
                OfferParams::builder()
                    .ramp_up_period(ramp_up)
                    .lock_timeout(lock_timeout)
                    .timeout(timeout)
                    .max_price(parse_ether("0.0005").unwrap_or_default() * U256::from(m_cycles))
                    .min_price(parse_ether("0.0001").unwrap_or_default() * U256::from(m_cycles))
                    .lock_stake(U256::from(m_cycles * 100)),
            );

        // Build the request, including preflight, and assigned the remaining fields.
        let request = boundless_client
            .build_request(request)
            .await
            .map_err(|e| AgentError::AgentError(format!("Failed to build request: {e}")))?;
        tracing::info!("Request: {:?}", request);

        let offchain = false;
        // Send the request and wait for it to be completed.
        let (request_id, expires_at) = if offchain {
            unimplemented!("offchain is not supported");
        } else {
            boundless_client
                .submit_request_onchain(&request)
                .await
                .map_err(|e| {
                    AgentError::AgentError(format!("Failed to submit request onchain: {e}"))
                })?
        };
        tracing::info!("Request 0x{request_id:x} submitted");

        // Wait for the request to be fulfilled by the market, returning the journal and seal.
        tracing::info!("Waiting for 0x{request_id:x} to be fulfilled");
        let (journal, seal) = boundless_client
            .wait_for_request_fulfillment(request_id, Duration::from_secs(10), expires_at)
            .await
            .map_err(|e| {
                AgentError::AgentError(format!("Failed to wait for request fulfillment: {e}"))
            })?;
        tracing::info!(
            "Request 0x{request_id:x} fulfilled. Journal: {:?}, Seal: {:?}, image_id: {:?}",
            journal,
            seal,
            BOUNDLESS_BATCH_ID,
        );

        let Ok(ContractReceipt::Base(boundless_receipt)) =
            risc0_ethereum_contracts_boundless::receipt::decode_seal(
                seal.clone(),
                BOUNDLESS_BATCH_ID,
                journal.clone(),
            )
        else {
            return Err(AgentError::AgentError(
                "did not receive requested unaggregated receipt".to_string(),
            ));
        };

        let response = Risc0Response {
            seal: seal.to_vec(),
            journal: journal.to_vec(),
            receipt: serde_json::to_string(&boundless_receipt).ok(),
        };
        // Use bincode to serialize the response and return as Vec<u8>
        let proof_bytes = bincode::serialize(&response)
            .map_err(|e| AgentError::AgentError(format!("Failed to encode response: {e}")))?;
        return Ok(proof_bytes);
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use super::*;
    use alloy_primitives_v1p2p0::hex;
    use env_logger;
    use ethers_contract::abigen;
    use ethers_core::types::H160;
    use ethers_providers::{Http, Provider, RetryClient};
    use log::{error as tracing_err, info as tracing_info};
    use risc0_zkvm::sha::Digestible;
    // use boundless_market::alloy::providers::Provider as BoundlessProvider;

    abigen!(
        IRiscZeroVerifier,
        r#"[
            function verify(bytes calldata seal, bytes32 imageId, bytes32 journalDigest) external view
        ]"#
    );

    #[tokio::test]
    async fn test_batch_run() {
        Risc0BoundlessProver::init_prover().await.unwrap();
    }

    #[tokio::test]
    async fn test_run_prover() {
        // init log
        env_logger::init();

        // loading from tests/fixtures/input-1306738.bin
        let input_bytes = std::fs::read("tests/fixtures/input-1306738.bin").unwrap();
        let output_bytes = std::fs::read("tests/fixtures/output-1306738.bin").unwrap();

        let config = serde_json::Value::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover
            .batch_run(input_bytes, &output_bytes, &config)
            .await
            .unwrap();
        println!("proof: {:?}", proof);

        let response: Risc0Response = bincode::deserialize(&proof).unwrap();
        println!("response: {:?}", response);

        // Save the proof to a binary file for inspection
        let bin_path = "tests/fixtures/proof-1306738.bin";
        std::fs::write(bin_path, &proof).expect("Failed to write proof to bin file");
        println!("Proof saved to {}", bin_path);
    }

    #[test]
    fn test_deserialize_zkvm_receipt() {
        // let file_name = format!("tests/fixtures/boundless_receipt_test.json");
        let file_name = format!("tests/fixtures/proof-1306738.bin");
        let bincode_proof: Vec<u8> = std::fs::read(file_name).unwrap();
        let proof: Risc0Response = bincode::deserialize(&bincode_proof).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let zkvm_receipt: ZkvmReceipt = serde_json::from_str(&proof.receipt.unwrap()).unwrap();
        println!("Deserialized zkvm receipt: {:#?}", zkvm_receipt);
    }

    #[tokio::test]
    async fn test_run_prover_aggregation() {
        env_logger::init();

        let file_name = format!("tests/fixtures/proof-1306738.bin");
        let proof: Vec<u8> = std::fs::read(file_name).unwrap();
        let proof: Risc0Response = bincode::deserialize(&proof).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let zkvm_receipt: ZkvmReceipt = serde_json::from_str(&proof.receipt.unwrap()).unwrap();
        let input_data = BoundlessAggregationGuestInput {
            image_id: BOUNDLESS_BATCH_ID.into(),
            receipts: vec![zkvm_receipt],
        };
        let input = bincode::serialize(&input_data).unwrap();
        let output = Vec::<u8>::new();
        let config = serde_json::Value::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover.aggregate(input, &output, &config).await.unwrap();
        println!("proof: {:?}", proof);
    }

    pub async fn verify_boundless_groth16_snark_impl(
        image_id: Digest,
        seal: Vec<u8>,
        journal_digest: Digest,
    ) -> bool {
        let verifier_rpc_url =
            std::env::var("GROTH16_VERIFIER_RPC_URL").expect("env GROTH16_VERIFIER_RPC_URL");
        let groth16_verifier_addr = {
            let addr =
                std::env::var("GROTH16_VERIFIER_ADDRESS").expect("env GROTH16_VERIFIER_RPC_URL");
            H160::from_str(&addr).unwrap()
        };

        let http_client = Arc::new(
            Provider::<RetryClient<Http>>::new_client(&verifier_rpc_url, 3, 500)
                .expect("Failed to create http client"),
        );

        tracing_info!("Verifying SNARK:");
        tracing_info!("Seal: {}", hex::encode(&seal));
        tracing_info!("Image ID: {}", hex::encode(image_id.as_bytes()));
        tracing_info!("Journal Digest: {}", hex::encode(journal_digest));
        // Fix: Use Arc for http_client to satisfy trait bounds for Provider
        let verify_call_res =
            IRiscZeroVerifier::new(groth16_verifier_addr, Arc::clone(&http_client))
                .verify(
                    seal.clone().into(),
                    image_id.as_bytes().try_into().unwrap(),
                    journal_digest.into(),
                )
                .await;

        if verify_call_res.is_ok() {
            tracing_info!("SNARK verified successfully using {groth16_verifier_addr:?}!");
            return true;
        } else {
            tracing_err!(
                "SNARK verification call to {groth16_verifier_addr:?} failed: {verify_call_res:?}!"
            );
            return false;
        }
    }

    #[test]
    fn test_image_id() {
        let image_id = risc0_zkvm::compute_image_id(BOUNDLESS_BATCH_ELF).unwrap();
        println!("image_id: {:?}", image_id);
        let image_id_bytes = BOUNDLESS_BATCH_ID
            .iter()
            .map(|x| x.to_le_bytes())
            .flatten()
            .collect::<Vec<u8>>();
        println!("image_id_bytes: {:?}", image_id_bytes);
        assert_eq!(
            image_id.as_bytes(),
            image_id_bytes,
            "Image IDs do not match"
        );
    }

    #[tokio::test]
    async fn test_verify_eth_receipt() {
        env_logger::try_init().ok();

        // Load a proof file and deserialize to Risc0Response
        let file_name = format!("tests/fixtures/proof-1306738.bin");
        let proof_bytes: Vec<u8> = std::fs::read(file_name).expect("Failed to read proof file");
        let proof: Risc0Response =
            bincode::deserialize(&proof_bytes).expect("Failed to deserialize proof");

        // Call the simulated onchain verification
        let journal_digest = proof.journal.digest();
        let verified = verify_boundless_groth16_snark_impl(
            BOUNDLESS_BATCH_ID.into(),
            proof.seal,
            journal_digest,
        )
        .await;
        assert!(verified, "Receipt failed onchain verification");
        println!("Onchain verification result: {}", verified);
    }
}
