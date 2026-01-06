use crate::types::{
    AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput, GuestInput,
    GuestOutput, IdStore, IdWrite, Proof, ProofKey, ProverError, ProverResult,
    ShastaAggregationGuestInput, ShastaZiskAggregationGuestInput, ZkAggregationGuestInput,
};
use alloy_primitives::{Address, B256};
use raiko_lib::{
    libhash::hash_shasta_subproof_input,
    primitives::keccak::keccak,
    proof_type::ProofType as RaikoProofType,
    prover::{
        IdStore as RaikoIdStore, IdWrite as RaikoIdWrite, Prover as RaikoProver, ProverConfig,
        ProverResult as RaikoProverResult, ProofKey as RaikoProofKey,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::sleep as tokio_async_sleep;
use tracing::info;

const ZISK_BATCH_ELF: &[u8] = include_bytes!("../../guest/elf/zisk-batch");
const ZISK_AGG_ELF: &[u8] = include_bytes!("../../guest/elf/zisk-aggregation");
const ZISK_SHASTA_AGG_ELF: &[u8] = include_bytes!("../../guest/elf/zisk-shasta-aggregation");

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ZiskAgentResponse {
    pub proof: Option<String>,
    pub receipt: Option<String>,
    pub input: Option<[u8; 32]>, // B256 equivalent
    pub uuid: Option<String>,
}

impl From<ZiskAgentResponse> for Proof {
    fn from(value: ZiskAgentResponse) -> Self {
        Self {
            proof: value.proof,
            quote: value.receipt,
            input: value.input.map(B256::from),
            uuid: value.uuid,
            kzg_proof: None,
            extra_data: None,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
enum AgentProofType {
    Batch,
    Aggregate,
}

#[derive(Debug, Serialize)]
struct AsyncProofRequestData {
    prover_type: &'static str,
    input: Vec<u8>,
    output: Vec<u8>,
    proof_type: AgentProofType,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct AsyncProofResponse {
    request_id: String,
}

#[derive(Debug, Deserialize)]
struct DetailedStatusResponse {
    status: String,
    status_message: String,
    proof_data: Option<Vec<u8>>,
    error: Option<String>,
    provider_request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageInfoResponse {
    provers: Vec<ProverImages>,
}

#[derive(Debug, Deserialize)]
struct ProverImages {
    prover_type: String,
    batch: Option<ImageDetails>,
    aggregation: Option<ImageDetails>,
}

#[derive(Debug, Deserialize)]
struct ImageDetails {
    uploaded: bool,
    elf_size_bytes: usize,
}

fn agent_auth_error(status: reqwest::StatusCode) -> Option<String> {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        Some(
            "Raiko agent rejected API key (missing or invalid). Set RAIKO_AGENT_API_KEY."
                .to_string(),
        )
    } else {
        None
    }
}

pub struct ZiskProverConfig {
    /// Maximum number of concurrent HTTP requests to the agent
    pub request_concurrency_limit: usize,
    /// Polling interval in seconds for checking proof status
    pub status_poll_interval_secs: u64,
    /// Maximum timeout in seconds for waiting for proof completion
    pub max_proof_timeout_secs: u64,
    /// Maximum retry attempts for status endpoint calls
    pub max_status_retries: u32,
    /// Retry delay in seconds between status endpoint attempts
    pub status_retry_delay_secs: u64,
    /// HTTP connection timeout in seconds
    pub http_connect_timeout_secs: u64,
    /// HTTP request timeout in seconds (applies to both POST and GET)
    pub http_timeout_secs: u64,
}

impl Default for ZiskProverConfig {
    fn default() -> Self {
        Self {
            request_concurrency_limit: 4,
            status_poll_interval_secs: 10,
            max_proof_timeout_secs: 3600,
            max_status_retries: 8,
            status_retry_delay_secs: 10,
            http_connect_timeout_secs: 10,
            http_timeout_secs: 60,
        }
    }
}

impl ZiskProverConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();

        Self {
            request_concurrency_limit: std::env::var("ZISK_REQUEST_CONCURRENCY_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.request_concurrency_limit),
            status_poll_interval_secs: std::env::var("ZISK_STATUS_POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.status_poll_interval_secs),
            max_proof_timeout_secs: std::env::var("ZISK_MAX_PROOF_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.max_proof_timeout_secs),
            max_status_retries: std::env::var("ZISK_MAX_STATUS_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.max_status_retries),
            status_retry_delay_secs: std::env::var("ZISK_STATUS_RETRY_DELAY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.status_retry_delay_secs),
            http_connect_timeout_secs: std::env::var("ZISK_HTTP_CONNECT_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.http_connect_timeout_secs),
            http_timeout_secs: std::env::var("ZISK_HTTP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.http_timeout_secs),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AggType {
    Base,
    Shasta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageType {
    Batch,
    Aggregation(AggType),
}

#[derive(Default, Debug, Clone, Copy)]
struct ImagesUploaded {
    batch: bool,
    aggregation: Option<AggType>,
}

struct ZiskAgentClient {
    remote_prover_url: String,
    api_key: Option<String>,
    request_semaphore: Arc<Semaphore>,
    config: ZiskProverConfig,
    images_uploaded: Arc<RwLock<ImagesUploaded>>,
}

impl ZiskAgentClient {
    fn new() -> Self {
        let remote_prover_url = std::env::var("ZISK_AGENT_URL")
            .or_else(|_| std::env::var("RAIKO_AGENT_URL"))
            .unwrap_or_else(|_| "http://localhost:9999/proof".to_string());
        let api_key = std::env::var("RAIKO_AGENT_API_KEY")
            .ok()
            .or_else(|| std::env::var("ZISK_AGENT_API_KEY").ok());
        let api_key = api_key.filter(|key| !key.is_empty());
        let config = ZiskProverConfig::from_env();

        Self {
            remote_prover_url,
            api_key,
            request_semaphore: Arc::new(Semaphore::new(config.request_concurrency_limit)),
            config,
            images_uploaded: Arc::new(RwLock::new(ImagesUploaded::default())),
        }
    }

    fn build_http_client(&self) -> ProverResult<reqwest::Client> {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(self.config.http_timeout_secs))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))
    }

    fn with_api_key(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.api_key.as_deref() {
            Some(key) if !key.is_empty() => builder.header("x-api-key", key),
            _ => builder,
        }
    }

    async fn verify_images_in_agent(
        &self,
        expected_batch_size: Option<usize>,
        expected_agg_size: Option<usize>,
    ) -> ProverResult<bool> {
        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let info_url = format!("{}/images", base_url);

        let client = self.build_http_client()?;
        let resp = self
            .with_api_key(client.get(&info_url))
            .send()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to query agent image info: {e}")))?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            return Ok(false);
        }

        let resp_json: ImageInfoResponse = resp.json().await.map_err(|e| {
            ProverError::GuestError(format!("Failed to parse images response: {e}"))
        })?;

        let zisk_info = resp_json
            .provers
            .into_iter()
            .find(|p| p.prover_type == "zisk");

        let batch_ok = if let Some(expected) = expected_batch_size {
            zisk_info
                .as_ref()
                .and_then(|info| info.batch.as_ref())
                .map(|details| details.uploaded && details.elf_size_bytes == expected)
                .unwrap_or(false)
        } else {
            true
        };

        let agg_ok = if let Some(expected) = expected_agg_size {
            zisk_info
                .as_ref()
                .and_then(|info| info.aggregation.as_ref())
                .map(|details| details.uploaded && details.elf_size_bytes == expected)
                .unwrap_or(false)
        } else {
            true
        };

        Ok(batch_ok && agg_ok)
    }

    async fn ensure_batch_uploaded(&self) -> ProverResult<()> {
        self.ensure_uploaded(ImageType::Batch).await
    }

    async fn ensure_base_agg_uploaded(&self) -> ProverResult<()> {
        self.ensure_uploaded(ImageType::Aggregation(AggType::Base))
            .await
    }

    async fn ensure_shasta_agg_uploaded(&self) -> ProverResult<()> {
        self.ensure_uploaded(ImageType::Aggregation(AggType::Shasta))
            .await
    }

    async fn ensure_uploaded(&self, image_type: ImageType) -> ProverResult<()> {
        let (expected_batch, expected_agg, upload_type, elf_bytes) = match image_type {
            ImageType::Batch => (
                Some(ZISK_BATCH_ELF.len()),
                None,
                "batch",
                ZISK_BATCH_ELF,
            ),
            ImageType::Aggregation(AggType::Base) => (
                None,
                Some(ZISK_AGG_ELF.len()),
                "aggregation",
                ZISK_AGG_ELF,
            ),
            ImageType::Aggregation(AggType::Shasta) => (
                None,
                Some(ZISK_SHASTA_AGG_ELF.len()),
                "aggregation",
                ZISK_SHASTA_AGG_ELF,
            ),
        };

        {
            let state = self.images_uploaded.read().await;
            let already_uploaded = match image_type {
                ImageType::Batch => state.batch,
                ImageType::Aggregation(agg) => state.aggregation == Some(agg),
            };
            if already_uploaded {
                return Ok(());
            }
        }

        if self
            .verify_images_in_agent(expected_batch, expected_agg)
            .await?
        {
            let mut state = self.images_uploaded.write().await;
            match image_type {
                ImageType::Batch => state.batch = true,
                ImageType::Aggregation(agg) => state.aggregation = Some(agg),
            }
            return Ok(());
        }

        self.upload_image_to_agent(upload_type, elf_bytes).await?;

        let mut state = self.images_uploaded.write().await;
        match image_type {
            ImageType::Batch => state.batch = true,
            ImageType::Aggregation(agg) => state.aggregation = Some(agg),
        }
        Ok(())
    }

    async fn upload_image_to_agent(
        &self,
        image_type: &str,
        elf_bytes: &[u8],
    ) -> ProverResult<()> {
        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let upload_url = format!("{}/upload-image/zisk/{}", base_url, image_type);

        let client = self.build_http_client()?;
        let resp = self
            .with_api_key(client.post(&upload_url))
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(elf_bytes.to_vec())
            .send()
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to upload {} image: {e}", image_type))
            })?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ProverError::GuestError(format!(
                "Agent returned error status {}: {}",
                status,
                body
            )));
        }

        Ok(())
    }

    async fn submit_request(&self, request: AsyncProofRequestData) -> ProverResult<String> {
        let client = self.build_http_client()?;
        let _permit = self.request_semaphore.acquire().await.map_err(|e| {
            ProverError::GuestError(format!("Failed to acquire request semaphore: {e}"))
        })?;

        let resp = self
            .with_api_key(client.post(&self.remote_prover_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to send request to agent: {e}")))?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            return Err(ProverError::GuestError(format!(
                "Agent returned error status: {}",
                resp.status()
            )));
        }

        let resp_json: AsyncProofResponse = resp
            .json()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to parse agent response: {e}")))?;

        Ok(resp_json.request_id)
    }

    async fn wait_for_proof(&self, request_id: String) -> ProverResult<Vec<u8>> {
        info!(
            "Waiting for zisk proof completion, polling agent status for request: {}",
            request_id
        );

        let max_retries = self.config.max_status_retries;
        let poll_interval = Duration::from_secs(self.config.status_poll_interval_secs);
        let max_timeout = Duration::from_secs(self.config.max_proof_timeout_secs);
        let start_time = std::time::Instant::now();

        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let status_url = format!("{}/status/{}", base_url, request_id);
        let client = self.build_http_client()?;

        loop {
            if start_time.elapsed() > max_timeout {
                return Err(ProverError::GuestError(format!(
                    "Zisk proof request {} timed out after {} seconds",
                    request_id, self.config.max_proof_timeout_secs
                )));
            }

            let mut res = None;
            for attempt in 1..=max_retries {
                let req = self.with_api_key(client.get(&status_url));

                match req.send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            match response.json::<DetailedStatusResponse>().await {
                                Ok(json_res) => {
                                    res = Some(json_res);
                                    break;
                                }
                                Err(err) => {
                                    if attempt == max_retries {
                                        return Err(ProverError::GuestError(format!(
                                            "Failed to parse status response: {}",
                                            err
                                        )));
                                    }
                                    tokio_async_sleep(Duration::from_secs(
                                        self.config.status_retry_delay_secs,
                                    ))
                                    .await;
                                    continue;
                                }
                            }
                        } else {
                            if let Some(message) = agent_auth_error(response.status()) {
                                return Err(ProverError::GuestError(message));
                            }
                            if attempt == max_retries {
                                return Err(ProverError::GuestError(format!(
                                    "Agent status endpoint error after {} attempts: {}",
                                    max_retries,
                                    response.status()
                                )));
                            }
                            tokio_async_sleep(Duration::from_secs(
                                self.config.status_retry_delay_secs,
                            ))
                            .await;
                            continue;
                        }
                    }
                    Err(err) => {
                        if attempt == max_retries {
                            return Err(ProverError::GuestError(format!(
                                "Failed to query agent status endpoint after {} attempts: {}",
                                max_retries, err
                            )));
                        }
                        tokio_async_sleep(Duration::from_secs(self.config.status_retry_delay_secs))
                            .await;
                        continue;
                    }
                }
            }

            let res = res.ok_or_else(|| ProverError::GuestError("status result not found".to_string()))?;
            let status = res.status.as_str();
            let status_message = res.status_message.as_str();

            let display_id = res
                .provider_request_id
                .as_deref()
                .map(|id| format!("provider request {}", id))
                .unwrap_or_else(|| format!("request {}", request_id));

            match status {
                "completed" => {
                    let proof_data = res.proof_data.ok_or_else(|| {
                        ProverError::GuestError("Missing proof data in completed status".to_string())
                    })?;
                    return Ok(proof_data);
                }
                "failed" => {
                    let error_detail = res
                        .error
                        .unwrap_or_else(|| "Unknown error".to_string());
                    return Err(ProverError::GuestError(format!(
                        "Zisk {} failed - {}: {}",
                        display_id, status_message, error_detail
                    )));
                }
                _ => {
                    info!("Zisk {}: {}", display_id, status_message);
                }
            }

            tokio_async_sleep(poll_interval).await;
        }
    }
}

fn compute_batch_image_id() -> [u32; 8] {
    let hash = keccak(ZISK_BATCH_ELF);
    let mut image_id = [0u32; 8];
    for (i, chunk) in hash.chunks(4).enumerate().take(8) {
        image_id[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    image_id
}

pub struct ZiskAgentProver;

impl ZiskAgentProver {
    pub async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("no block run after pacaya fork")
    }

    pub async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        info!("Zisk agent batch proof starting");

        let client = ZiskAgentClient::new();
        client.ensure_batch_uploaded().await?;

        let serialized_input = bincode::serialize(&input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize GuestBatchInput: {e}")))?;

        let output_bytes: Vec<u8> = output.hash.as_ref().to_vec();

        let request_id = client
            .submit_request(AsyncProofRequestData {
                prover_type: "zisk",
                input: serialized_input,
                output: output_bytes,
                proof_type: AgentProofType::Batch,
                config: None,
            })
            .await?;

        let proof_bytes = client.wait_for_proof(request_id).await?;
        let agent_response: ZiskAgentResponse = bincode::deserialize(&proof_bytes)?;

        let mut proof: Proof = agent_response.into();
        if proof.input.is_none() {
            proof.input = Some(output.hash);
        }

        Ok(proof)
    }

    pub async fn aggregate(
        &self,
        input: AggregationGuestInput,
        output: &AggregationGuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        info!("Zisk agent aggregation proof starting");

        let client = ZiskAgentClient::new();
        client.ensure_base_agg_uploaded().await?;

        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.input.ok_or_else(|| {
                    ProverError::GuestError(format!(
                        "Proof {} input is None for aggregation",
                        i
                    ))
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;

        let zisk_input = ZkAggregationGuestInput {
            image_id: compute_batch_image_id(),
            block_inputs,
        };

        let serialized_input = bincode::serialize(&zisk_input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize aggregation input: {e}")))?;

        let output_bytes: Vec<u8> = output.hash.as_ref().to_vec();

        let request_id = client
            .submit_request(AsyncProofRequestData {
                prover_type: "zisk",
                input: serialized_input,
                output: output_bytes,
                proof_type: AgentProofType::Aggregate,
                config: None,
            })
            .await?;

        let proof_bytes = client.wait_for_proof(request_id).await?;
        let agent_response: ZiskAgentResponse = bincode::deserialize(&proof_bytes)?;

        Ok(agent_response.into())
    }

    pub async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        output: &AggregationGuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        info!("Zisk agent shasta aggregation proof starting");

        let client = ZiskAgentClient::new();
        client.ensure_shasta_agg_uploaded().await?;

        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.input.ok_or_else(|| {
                    ProverError::GuestError(format!(
                        "Proof {} input is None for shasta aggregation",
                        i
                    ))
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;

        let proof_carry_data_vec = input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.extra_data.clone().ok_or_else(|| {
                    ProverError::GuestError(format!(
                        "Proof {} missing shasta proof carry data",
                        i
                    ))
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;

        if block_inputs.len() != proof_carry_data_vec.len() {
            return Err(ProverError::GuestError(format!(
                "Shasta aggregation input length mismatch: {} block inputs vs {} carry records",
                block_inputs.len(),
                proof_carry_data_vec.len()
            )));
        }

        for (i, block_input) in block_inputs.iter().enumerate() {
            let expected = hash_shasta_subproof_input(&proof_carry_data_vec[i]);
            if *block_input != expected {
                return Err(ProverError::GuestError(format!(
                    "Shasta aggregation block input {} does not match proof carry data",
                    i
                )));
            }
        }

        let shasta_input = ShastaZiskAggregationGuestInput {
            image_id: compute_batch_image_id(),
            block_inputs,
            proof_carry_data_vec,
            prover_address: Address::ZERO,
        };

        let serialized_input = bincode::serialize(&shasta_input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize shasta input: {e}")))?;

        let output_bytes: Vec<u8> = output.hash.as_ref().to_vec();

        let request_id = client
            .submit_request(AsyncProofRequestData {
                prover_type: "zisk",
                input: serialized_input,
                output: output_bytes,
                proof_type: AgentProofType::Aggregate,
                config: None,
            })
            .await?;

        let proof_bytes = client.wait_for_proof(request_id).await?;
        let agent_response: ZiskAgentResponse = bincode::deserialize(&proof_bytes)?;

        Ok(agent_response.into())
    }

    pub async fn cancel(
        &self,
        _proof_key: ProofKey,
        _id_store: Box<&mut dyn IdStore>,
    ) -> ProverResult<()> {
        info!("Zisk agent cancel requested - not implemented");
        Ok(())
    }
}

impl RaikoProver for ZiskAgentProver {
    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn RaikoIdWrite>,
    ) -> RaikoProverResult<Proof> {
        ZiskAgentProver::run(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn RaikoIdWrite>,
    ) -> RaikoProverResult<Proof> {
        ZiskAgentProver::batch_run(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn RaikoIdWrite>,
    ) -> RaikoProverResult<Proof> {
        ZiskAgentProver::aggregate(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn shasta_aggregate(
        &self,
        input: raiko_lib::input::ShastaAggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn RaikoIdWrite>,
    ) -> RaikoProverResult<Proof> {
        ZiskAgentProver::shasta_aggregate(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn cancel(
        &self,
        _proof_key: RaikoProofKey,
        _read: Box<&mut dyn RaikoIdStore>,
    ) -> RaikoProverResult<()> {
        Ok(())
    }

    fn proof_type(&self) -> RaikoProofType {
        RaikoProofType::Zisk
    }
}
