#![cfg(feature = "enable")]

use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, RawAggregationGuestInput, RawProof,
    },
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{SgxParam, SgxResponse};

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RemoteSgxResponse {
    pub status: String,
    pub message: String,
    #[serde(alias = "proof")]
    pub sgx_response: SgxResponse,
}

// raiko end point
const RAIKO_REMOTE_URL: &str = "http://localhost:9090";
// gaiko end point
const GAIKO_REMOTE_URL: &str = "http://localhost:8090";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteSgxProver {
    proof_type: ProofType,
    remote_prover_url: String,
}

impl RemoteSgxProver {
    pub fn new(proof_type: ProofType) -> Self {
        let remote_prover_url =
            match proof_type {
                ProofType::SgxGeth => std::env::var("GAIKO_REMOTE_URL")
                    .unwrap_or_else(|_| GAIKO_REMOTE_URL.to_string()),
                ProofType::Sgx => std::env::var("RAIKO_REMOTE_URL")
                    .unwrap_or_else(|_| RAIKO_REMOTE_URL.to_string()),
                _ => panic!("Unsupported proof type for remote prover"),
            };
        Self {
            proof_type,
            remote_prover_url,
        }
    }
}

impl Prover for RemoteSgxProver {
    async fn run(
        &self,
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            unimplemented!("SGX setup not implemented for remote prover");
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(&self.remote_prover_url, self.proof_type).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input, &sgx_param)?;
            sgx_proof = prove(&self.remote_prover_url, input.clone(), instance_id).await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            unimplemented!("SGX setup not implemented for remote prover");
        }

        if sgx_param.bootstrap {
            unimplemented!("SGX bootstrap not implemented for aggregation request");
        };

        println!("input: {:?}", input);

        let sgx_proof = aggregate(&self.remote_prover_url, input.clone(), self.proof_type).await?;
        Ok(sgx_proof.into())
    }

    async fn cancel(&self, _proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            unimplemented!("SGX setup not implemented for remote prover");
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(&self.remote_prover_url, self.proof_type).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input.inputs[0], &sgx_param)?;
            sgx_proof = batch_prove(
                &self.remote_prover_url,
                input.clone(),
                instance_id,
                self.proof_type,
            )
            .await
        }

        sgx_proof.map(|r| r.into())
    }
}

pub async fn check_bootstrap(
    remote_sgx_url: &str,
    _proof_type: ProofType,
) -> ProverResult<(), ProverError> {
    // post to remote sgx provider/bootstrap
    let client: Client = Client::new();
    let remote_post_url = format!("{}/check", remote_sgx_url);
    let response = client
        .post(remote_post_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        tracing::info!("Response: {}", response_text);
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(())
        } else {
            tracing::error!("Request failed with status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Failed to read error response: {}",
                sgx_proof.message
            )))
        }
    } else {
        tracing::error!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Failed to read error response: {}",
            response.status()
        )))
    }
}

pub async fn bootstrap(
    remote_sgx_url: &str,
    _proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/bootstrap", remote_sgx_url);
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        println!("Response: {}", response_text);
        serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))
    } else {
        println!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Failed to read error response: {}",
            response.status()
        )))
    }
}

async fn prove(
    remote_sgx_url: &str,
    input: GuestInput,
    _instance_id: u64,
) -> ProverResult<SgxResponse, ProverError> {
    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/prove/block", remote_sgx_url);
    let json_input = serde_json::to_string(&input)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(json_input)
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        println!("Response: {}", response_text);
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            tracing::error!("Request failed with status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Failed to read error response: {}",
                sgx_proof.message
            )))
        }
    } else {
        println!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Failed to read error response: {}",
            response.status()
        )))
    }
}

async fn batch_prove(
    remote_sgx_url: &str,
    input: GuestBatchInput,
    _instance_id: u64,
    _proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/prove/batch", remote_sgx_url);
    let json_input = serde_json::to_string(&input)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(json_input)
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        tracing::info!("Response: {}", response_text);
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            tracing::error!("Request failed with status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Failed to read error response: {}",
                sgx_proof.message
            )))
        }
    } else {
        tracing::error!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Failed to read error response: {}",
            response.status()
        )))
    }
}

async fn aggregate(
    remote_sgx_url: &str,
    input: AggregationGuestInput,
    _proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // Extract the useful parts of the proof here so the guest doesn't have to do it
    let raw_input = RawAggregationGuestInput {
        proofs: input
            .proofs
            .iter()
            .map(|proof| RawProof {
                input: proof.clone().input.unwrap(),
                proof: hex::decode(&proof.clone().proof.unwrap()[2..]).unwrap(),
            })
            .collect(),
    };
    // Extract the instance id from the first proof
    let _instance_id = {
        let mut instance_id_bytes = [0u8; 4];
        instance_id_bytes[0..4].copy_from_slice(&raw_input.proofs[0].proof.clone()[0..4]);
        u32::from_be_bytes(instance_id_bytes)
    };

    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/prove/aggregate", remote_sgx_url);
    let json_input = serde_json::to_string(&raw_input)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(json_input)
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        tracing::info!("Response: {}", response_text);
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            tracing::error!("Request failed with status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Failed to read error response: {}",
                sgx_proof.message
            )))
        }
    } else {
        tracing::error!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Failed to read error response: {}",
            response.status()
        )))
    }
}

pub fn get_instance_id_from_params(input: &GuestInput, sgx_param: &SgxParam) -> ProverResult<u64> {
    let chain = input.chains.get(&input.taiko.parent_chain_id).unwrap();
    let spec_id = chain
        .chain_spec
        .active_fork(chain.block.number, chain.block.timestamp)
        .map_err(|e| ProverError::GuestError(e.to_string()))?;
    sgx_param
        .instance_ids
        .get(&spec_id)
        .cloned()
        .ok_or_else(|| {
            ProverError::GuestError(format!("No instance id found for spec id: {:?}", spec_id))
        })
}
