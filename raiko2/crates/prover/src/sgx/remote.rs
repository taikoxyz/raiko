use std::{
    collections::HashMap,
    panic::{self, AssertUnwindSafe},
};

use raiko_lib::input::{AggregationGuestInput, RawAggregationGuestInput, RawProof};
use raiko_lib::primitives::B256;
use raiko2_primitives::{GuestInput, Proof, ProverConfig, ProverError, ProverResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tokio::time::Duration;
use tracing::{debug, error, info};

const RAIKO_REMOTE_URL: &str = "http://localhost:9090";
const CONFIG_KEY: &str = "sgx";

#[serde_as]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SgxParam {
    #[serde(default)]
    pub instance_id: Option<u64>,
    #[serde(default)]
    pub instance_ids: HashMap<u8, u64>,
    pub setup: bool,
    pub bootstrap: bool,
    pub prove: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
    pub input: B256,
}

impl From<SgxResponse> for Proof {
    fn from(value: SgxResponse) -> Self {
        Self {
            proof: Some(value.proof),
            input: Some(value.input),
            quote: Some(value.quote),
            uuid: None,
            kzg_proof: None,
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RemoteSgxResponse {
    pub status: String,
    pub message: String,
    #[serde(alias = "proof")]
    pub sgx_response: SgxResponse,
}

#[derive(Clone, Debug)]
pub struct RemoteSgxProver {
    remote_prover_url: String,
}

impl RemoteSgxProver {
    pub fn new() -> Self {
        let remote_prover_url =
            std::env::var("RAIKO_REMOTE_URL").unwrap_or_else(|_| RAIKO_REMOTE_URL.to_string());

        Self { remote_prover_url }
    }

    fn extract_params(&self, config: &ProverConfig) -> ProverResult<SgxParam> {
        let params = config
            .get(CONFIG_KEY)
            .cloned()
            .ok_or_else(|| ProverError::GuestError("missing SGX config".to_string()))?;
        serde_json::from_value(params).map_err(ProverError::Param)
    }

    fn parse_instance_id(&self, input: &GuestInput, params: &SgxParam) -> ProverResult<u64> {
        if let Some(id) = params.instance_id {
            return Ok(id);
        }
        if params.instance_ids.len() == 1 {
            return params.instance_ids.values().next().copied().ok_or_else(|| {
                ProverError::GuestError("SGX instance id configuration is empty".to_string())
            });
        }
        if params.instance_ids.is_empty() {
            return Err(ProverError::GuestError(
                "SGX instance id not configured".to_string(),
            ));
        }

        let first_input = input.witnesses.first().ok_or_else(|| {
            ProverError::GuestError("guest input witnesses are empty".to_string())
        })?;
        let block = &first_input.block;
        let number = block.header.number;
        let timestamp = block.header.timestamp;
        let spec_id = panic::catch_unwind(AssertUnwindSafe(|| {
            input.taiko.chain_spec.active_fork(number, timestamp)
        }))
        .ok()
        .and_then(Result::ok);

        if let Some(spec_id) = spec_id {
            params.instance_ids.get(&spec_id).cloned().ok_or_else(|| {
                ProverError::GuestError(format!("no SGX instance id configured for spec {spec_id}"))
            })
        } else {
            Err(ProverError::GuestError(
                "unable to determine SGX instance id for current fork".to_string(),
            ))
        }
    }

    pub async fn prove(&self, input: GuestInput, config: &ProverConfig) -> ProverResult<Proof> {
        let params = self.extract_params(config)?;

        if params.setup {
            return Err(ProverError::GuestError(
                "remote SGX prover setup not implemented".to_string(),
            ));
        }

        let mut proof = if params.bootstrap {
            info!("Running SGX bootstrap request");
            bootstrap(&self.remote_prover_url).await
        } else {
            Ok(SgxResponse::default())
        }?;

        if params.prove {
            let instance_id = self.parse_instance_id(&input, &params)?;
            info!(
                remote = %self.remote_prover_url,
                instance_id,
                "Dispatching SGX prove request",
            );
            proof = batch_prove(&self.remote_prover_url, &input).await?;
        }

        Ok(proof.into())
    }

    pub async fn aggregate(
        &self,
        input: AggregationGuestInput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let params = self.extract_params(config)?;

        if params.setup {
            return Err(ProverError::GuestError(
                "remote SGX setup not supported for aggregation".to_string(),
            ));
        }

        if params.bootstrap {
            return Err(ProverError::GuestError(
                "remote SGX bootstrap not supported for aggregation".to_string(),
            ));
        }

        let proof = aggregate(&self.remote_prover_url, input).await?;
        Ok(proof.into())
    }
}

impl Default for RemoteSgxProver {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn bootstrap(remote_sgx_url: &str) -> ProverResult<SgxResponse> {
    info!("Sending SGX bootstrap request");
    let client = Client::new();
    let post_url = format!("{remote_sgx_url}/bootstrap");
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|err| ProverError::GuestError(format!("Failed to send request: {err}")))?;

    if response.status().is_success() {
        let body = response
            .text()
            .await
            .map_err(|err| ProverError::GuestError(format!("Failed to read response: {err}")))?;
        debug!(body, "Received SGX bootstrap response");
        serde_json::from_str(&body)
            .map_err(|err| ProverError::GuestError(format!("Failed to parse response: {err}")))
    } else {
        error!(status = ?response.status(), "SGX bootstrap request failed");
        Err(ProverError::GuestError(format!(
            "Bootstrap failed with status: {}",
            response.status()
        )))
    }
}

async fn batch_prove(remote_sgx_url: &str, input: &GuestInput) -> ProverResult<SgxResponse> {
    let client = Client::new();
    let post_url = format!("{remote_sgx_url}/prove/batch");
    let payload = serde_json::to_vec(input)
        .map_err(|err| ProverError::GuestError(format!("Failed to serialize input: {err}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(payload)
        .timeout(Duration::from_secs(200))
        .send()
        .await
        .map_err(|err| ProverError::GuestError(format!("Failed to send request: {err}")))?;

    if response.status().is_success() {
        let body = response
            .text()
            .await
            .map_err(|err| ProverError::GuestError(format!("Failed to read response: {err}")))?;
        debug!(body, "Received SGX prove response");
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&body)
            .map_err(|err| ProverError::GuestError(format!("Failed to parse response: {err}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            error!(
                status = sgx_proof.status,
                message = sgx_proof.message,
                "SGX prove request failed"
            );
            Err(ProverError::GuestError(format!(
                "Prove failed: {}",
                sgx_proof.message
            )))
        }
    } else {
        error!(status = ?response.status(), "SGX prove request failed");
        Err(ProverError::GuestError(format!(
            "Prove failed with status: {}",
            response.status()
        )))
    }
}

async fn aggregate(
    remote_sgx_url: &str,
    input: AggregationGuestInput,
) -> ProverResult<SgxResponse> {
    let raw_input = RawAggregationGuestInput {
        proofs: input
            .proofs
            .into_iter()
            .map(|proof| {
                let proof_hex = proof
                    .proof
                    .ok_or_else(|| ProverError::GuestError("missing sgx proof".to_string()))?;
                let proof_bytes =
                    hex::decode(proof_hex.trim_start_matches("0x")).map_err(|err| {
                        ProverError::GuestError(format!("invalid sgx proof hex: {err}"))
                    })?;
                let input = proof
                    .input
                    .ok_or_else(|| ProverError::GuestError("missing input hash".to_string()))?;

                Ok(RawProof {
                    proof: proof_bytes,
                    input,
                })
            })
            .collect::<ProverResult<Vec<_>>>()?,
    };

    info!("Dispatching SGX aggregation request");
    let client = Client::new();
    let post_url = format!("{remote_sgx_url}/prove/aggregate");
    let payload = serde_json::to_vec(&raw_input)
        .map_err(|err| ProverError::GuestError(format!("Failed to serialize input: {err}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(payload)
        .timeout(Duration::from_secs(200))
        .send()
        .await
        .map_err(|err| ProverError::GuestError(format!("Failed to send request: {err}")))?;

    if response.status().is_success() {
        let body = response
            .text()
            .await
            .map_err(|err| ProverError::GuestError(format!("Failed to read response: {err}")))?;
        debug!(body, "Received SGX aggregation response");
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&body)
            .map_err(|err| ProverError::GuestError(format!("Failed to parse response: {err}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            error!(
                status = sgx_proof.status,
                message = sgx_proof.message,
                "SGX aggregation request failed"
            );
            Err(ProverError::GuestError(format!(
                "Aggregation failed: {}",
                sgx_proof.message
            )))
        }
    } else {
        error!(status = ?response.status(), "SGX aggregation request failed");
        Err(ProverError::GuestError(format!(
            "Aggregation failed with status: {}",
            response.status()
        )))
    }
}
