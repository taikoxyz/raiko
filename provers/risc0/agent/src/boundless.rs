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

#[derive(Clone, Serialize, Deserialize)]
pub struct Risc0Response {
    pub seal: Vec<u8>,
    pub journal: Vec<u8>,
}

pub struct Risc0BoundlessProver {
    batch_image_url: Option<Url>,
    aggregation_image_url: Option<Url>,
}

impl Risc0BoundlessProver {
    pub async fn get() -> &'static Self {
        get_boundless_prover().await
    }

    pub async fn init_prover() -> Result<Self, ProverError> {
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
                    ProverError::GuestError(format!("Failed to build boundless client: {e}"))
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
                ProverError::GuestError(format!("Failed to upload BOUNDLESS_BATCH_ELF image: {e}"))
            })?;

        let aggregation_image_url = boundless_client
            .upload_program(BOUNDLESS_AGGREGATION_ELF)
            .await
            .map_err(|e| {
                ProverError::GuestError(format!(
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
pub enum ProverError {
    #[error("Guest error: {0}")]
    GuestError(String),
}

pub type ProverResult<T> = Result<T, ProverError>;

impl Risc0BoundlessProver {
    pub async fn run(
        &self,
        _input: Vec<u8>,
        _output: &[u8],
        _config: &serde_json::Value,
    ) -> ProverResult<Vec<u8>> {
        unimplemented!("No need for post pacaya");
    }

    pub async fn aggregate(
        &self,
        _input: Vec<u8>,
        _output: &[u8],
        _config: &serde_json::Value,
    ) -> ProverResult<Vec<u8>> {
        todo!()
    }

    pub async fn cancel(&self, _key: (u64, u64, B256, u8)) -> ProverResult<()> {
        todo!()
    }

    pub async fn batch_run(
        &self,
        _input: Vec<u8>,
        _output: &[u8],
        _config: &serde_json::Value,
    ) -> ProverResult<Vec<u8>> {
        // Encode the input and upload it to the storage provider.
        let encoded_input = _input;
        let guest_env = GuestEnv::builder().write_frame(&encoded_input).build_env();
        let guest_env_bytes = guest_env.clone().encode().map_err(|e| {
            ProverError::GuestError(format!("Failed to encode guest environment: {e}"))
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
                    ProverError::GuestError(format!("Failed to execute guest environment: {e}"))
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
                    ProverError::GuestError(format!("Failed to build boundless client: {e}"))
                })?;
            client
        };

        let input_url = boundless_client
            .upload_input(&guest_env_bytes)
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to upload input: {e}")))?;
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
            .map_err(|e| ProverError::GuestError(format!("Failed to build request: {e}")))?;
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
                    ProverError::GuestError(format!("Failed to submit request onchain: {e}"))
                })?
        };
        tracing::info!("Request 0x{request_id:x} submitted");

        // Wait for the request to be fulfilled by the market, returning the journal and seal.
        tracing::info!("Waiting for 0x{request_id:x} to be fulfilled");
        let (journal, seal) = boundless_client
            .wait_for_request_fulfillment(request_id, Duration::from_secs(10), expires_at)
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to wait for request fulfillment: {e}"))
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
        };
        // Use bincode to serialize the response and return as Vec<u8>
        let proof_bytes = bincode::serialize(&response)
            .map_err(|e| ProverError::GuestError(format!("Failed to encode response: {e}")))?;
        return Ok(proof_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_logger;

    #[tokio::test]
    async fn test_batch_run() {
        Risc0BoundlessProver::init_prover().await.unwrap();
    }

    #[tokio::test]
    async fn test_run_prover() {
        // init log
        env_logger::init();

        // loading from ../../gaiko/tests/fixtures/batch/input-1306738.json
        let input_bytes = std::fs::read("../../../input-1306738.bin").unwrap();
        let output_bytes = std::fs::read("../../../output-1306738.bin").unwrap();

        let config = serde_json::Value::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover
            .batch_run(input_bytes, &output_bytes, &config)
            .await
            .unwrap();
        println!("proof: {:?}", proof);
    }

    #[test]
    fn test_deserialize_proof() {
        // This test deserializes a proof from a JSON string (as would be returned from Boundless).
        let proof_json = r#"{
             "proof": "0x0000000000000000000000000000000000000000000000000000000000000040a9b03d0dd651aebfd77634799760072e8392c3c91e17d7c3da6785a61aaffdbe00000000000000000000000000000000000000000000000000000000000001049f39696c117e359f6a322d19b2ea8437271cda231c152d70fb553c6ed68e5c90e05c307c2787e39785bdec77c7cd712005367690160274f270397d7eca1e103c5633f7711ea988975445d70d2ce30d4da7648aa55d311b3796ffb35b3648ee7dd848f150002db50185bbc16d3aacf2d5ea19fe9368361b57ebc8590df4f637a91a142a32200efe06906e1e33c0e2caa7e8e9bec6aa0289e7f4ccb771ababe0a7df5e82960633839ddff0e44685ad0b9f137da03fd51cbeccc3d6cd163c83395814ed3d9618aca53e3ec65562300fee630606e22fe2b84c70a63dd60ffc42781f4d49ca08016bbe2581766d96144b1c90eb1eb65cfba92e9b4353c1fb9a6e89b957e3c1bf00000000000000000000000000000000000000000000000000000000",
             "input": "0x6f478ee63e81d8f341716638ebb2c524884af8441de92aed284176210169e942",
             "quote": "{\"inner\":{\"Groth16\":{\"seal\":[17,126,53,159,106,50,45,25,178,234,132,55,39,28,218,35,28,21,45,112,251,85,60,110,214,142,92,144,224,92,48,124,39,135,227,151,133,189,236,119,199,205,113,32,5,54,118,144,22,2,116,242,112,57,125,126,202,30,16,60,86,51,247,113,30,169,136,151,84,69,215,13,44,227,13,77,167,100,138,165,93,49,27,55,150,255,179,91,54,72,238,125,216,72,241,80,0,45,181,1,133,187,193,109,58,172,242,213,234,25,254,147,104,54,27,87,235,200,89,13,244,246,55,169,26,20,42,50,32,14,254,6,144,110,30,51,192,226,202,167,232,233,190,198,170,2,137,231,244,204,183,113,171,171,224,167,223,94,130,150,6,51,131,157,223,240,228,70,133,173,11,159,19,125,160,63,213,28,190,204,195,214,205,22,60,131,57,88,20,237,61,150,24,172,165,62,62,198,85,98,48,15,238,99,6,6,226,47,226,184,76,112,166,61,214,15,252,66,120,31,77,73,202,8,1,107,190,37,129,118,109,150,20,75,28,144,235,30,182,92,251,169,46,155,67,83,193,251,154,110,137,185,87,227,193,191],\"claim\":{\"Value\":{\"pre\":{\"Pruned\":[222146729,3215872470,2033481431,772235415,3385037443,3285653278,2793760730,3204296474]},\"post\":{\"Value\":{\"pc\":0,\"merkle_root\":[0,0,0,0,0,0,0,0]}},\"exit_code\":{\"Halted\":0},\"input\":{\"Value\":null},\"output\":{\"Value\":{\"journal\":{\"Value\":[32,0,0,0,176,40,71,100,170,231,243,39,65,10,225,53,91,239,35,220,204,208,249,199,35,114,77,102,56,168,237,222,134,9,29,101]},\"assumptions\":{\"Pruned\":[0,0,0,0,0,0,0,0]}}}}},\"verifier_parameters\":[1818835359,1620946611,2780288568,2130774364,576647948,727242602,2964052866,2234770906]}},\"journal\":{\"bytes\":[32,0,0,0,176,40,71,100,170,231,243,39,65,10,225,53,91,239,35,220,204,208,249,199,35,114,77,102,56,168,237,222,134,9,29,101]},\"metadata\":{\"verifier_parameters\":[1818835359,1620946611,2780288568,2130774364,576647948,727242602,2964052866,2234770906]}}}",
             "uuid": "",
             "kzg_proof": null
         }"#;

        // The ContractReceipt type is used for Boundless receipts.
        let proof: Vec<u8> =
            serde_json::from_str(proof_json).expect("Failed to deserialize proof JSON");
        println!("Deserialized receipt: {:#?}", proof);
    }

    #[test]
    fn test_deserialize_zkvm_receipt() {
        // let file_name = format!("../../../boundless_receipt_test.json");
        let file_name = format!("../../../boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Vec<u8> = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let zkvm_receipt: ZkvmReceipt =
            serde_json::from_str(&String::from_utf8_lossy(&proof)).unwrap();
        println!("Deserialized zkvm receipt: {:#?}", zkvm_receipt);
    }

    #[tokio::test]
    async fn test_run_proof_aggregation() {
        env_logger::init();

        let file_name = format!("../../../boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Vec<u8> = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let input = Vec::<u8>::new(); // AggregationGuestInput as bytes
        let output = Vec::<u8>::new(); // AggregationGuestOutput as bytes
        let config = serde_json::Value::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover.aggregate(input, &output, &config).await.unwrap();
        println!("proof: {:?}", proof);
    }
}
