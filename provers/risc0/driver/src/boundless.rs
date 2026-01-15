use crate::{
    methods::{
        boundless_aggregation::{BOUNDLESS_AGGREGATION_ELF, BOUNDLESS_AGGREGATION_ID},
        boundless_batch::{BOUNDLESS_BATCH_ELF, BOUNDLESS_BATCH_ID},
        boundless_shasta_aggregation::{
            BOUNDLESS_SHASTA_AGGREGATION_ELF, BOUNDLESS_SHASTA_AGGREGATION_ID,
        },
    },
    snarks::verify_boundless_groth16_snark_impl,
    Risc0Response,
};
use alloy_primitives::B256;
use alloy_sol_types::SolValue;
use hex;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput,
    },
    libhash::hash_shasta_subproof_input,
    primitives::keccak::keccak,
    proof_type::ProofType,
    prover::{
        IdStore, IdWrite, Proof, ProofCarryData, ProofKey, Prover, ProverConfig, ProverError,
        ProverResult,
    },
    protocol_instance::validate_shasta_proof_carry_data_vec,
};
use risc0_zkvm::{compute_image_id, sha::Digestible, Digest, Receipt as ZkvmReceipt};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep as tokio_async_sleep;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Risc0AgentAggGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<ZkvmReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundlessShastaAggregationGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<ZkvmReceipt>,
    pub proof_carry_data_vec: Vec<ProofCarryData>,
}

// share with agent, need a unified place for this
// now just copy from agent
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Risc0AgentResponse {
    pub seal: Vec<u8>,
    pub journal: Vec<u8>,
    pub receipt: Option<String>,
}

/// Generate cache label: {image_id}-{keccak(input)}
fn cache_label(image_id: &Digest, input: &[u8]) -> String {
    format!("{}-{}", hex::encode(image_id), hex::encode(keccak(input)))
}

/// Save proof to /tmp/risc0-cache/{label}.boundless
fn save_proof(label: &str, proof: &Risc0AgentResponse) {
    let path = Path::new("/tmp/risc0-cache");
    let _ = fs::create_dir_all(path);
    let file = path.join(format!("{}.boundless", label));
    if let Ok(data) = bincode::serialize(proof) {
        let _ = fs::write(&file, data);
        tracing::info!("Saved boundless proof to cache: {:?}", file);
    }
}

/// Load proof from /tmp/risc0-cache/{label}.boundless
fn load_proof(label: &str) -> Option<Risc0AgentResponse> {
    let file = Path::new("/tmp/risc0-cache").join(format!("{}.boundless", label));
    fs::read(&file).ok()
        .and_then(|data| bincode::deserialize(&data).ok())
        .map(|proof| {
            tracing::info!("Loaded boundless proof from cache: {:?}", file);
            proof
        })
}

fn validate_shasta_inputs(
    proofs: &[Proof],
    proof_carry_data_vec: &[ProofCarryData],
) -> ProverResult<()> {
    if proofs.len() != proof_carry_data_vec.len() {
        return Err(ProverError::GuestError(
            "shasta proofs length mismatch with carry data".to_string(),
        ));
    }
    if !validate_shasta_proof_carry_data_vec(proof_carry_data_vec) {
        return Err(ProverError::GuestError(
            "invalid shasta proof carry data".to_string(),
        ));
    }

    for (idx, (proof, carry)) in proofs.iter().zip(proof_carry_data_vec).enumerate() {
        let proof_input = proof
            .input
            .ok_or_else(|| ProverError::GuestError("missing shasta proof public input".into()))?;
        let expected = hash_shasta_subproof_input(carry);
        if proof_input != expected {
            return Err(ProverError::GuestError(format!(
                "shasta proof input mismatch at index {idx}"
            )));
        }
    }

    Ok(())
}

fn agent_auth_error(status: reqwest::StatusCode) -> Option<String> {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        Some(
            "Boundless agent rejected API key (missing or invalid). Set RAIKO_AGENT_API_KEY."
                .to_string(),
        )
    } else {
        None
    }
}

pub struct BoundlessProverConfig {
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

impl Default for BoundlessProverConfig {
    fn default() -> Self {
        Self {
            request_concurrency_limit: 4,
            status_poll_interval_secs: 15,
            max_proof_timeout_secs: 3600, // 1 hour
            max_status_retries: 8,
            status_retry_delay_secs: 15,
            http_connect_timeout_secs: 10,
            http_timeout_secs: 60,
        }
    }
}

impl BoundlessProverConfig {
    /// Load configuration from environment variables, falling back to defaults
    pub fn from_env() -> Self {
        let defaults = Self::default();

        Self {
            request_concurrency_limit: std::env::var("BOUNDLESS_REQUEST_CONCURRENCY_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.request_concurrency_limit),
            status_poll_interval_secs: std::env::var("BOUNDLESS_STATUS_POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.status_poll_interval_secs),
            max_proof_timeout_secs: std::env::var("BOUNDLESS_MAX_PROOF_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.max_proof_timeout_secs),
            max_status_retries: std::env::var("BOUNDLESS_MAX_STATUS_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.max_status_retries),
            status_retry_delay_secs: std::env::var("BOUNDLESS_STATUS_RETRY_DELAY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.status_retry_delay_secs),
            http_connect_timeout_secs: std::env::var("BOUNDLESS_HTTP_CONNECT_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.http_connect_timeout_secs),
            http_timeout_secs: std::env::var("BOUNDLESS_HTTP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.http_timeout_secs),
        }
    }
}

pub struct BoundlessProver {
    remote_prover_url: String,
    api_key: Option<String>,
    request_semaphore: Arc<Semaphore>,
    config: BoundlessProverConfig,
    images_uploaded: Arc<tokio::sync::RwLock<ImagesUploaded>>,
    auth_checked: AtomicBool,
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

impl BoundlessProver {
    pub fn new() -> Self {
        let remote_prover_url = std::env::var("RAIKO_AGENT_URL")
            .unwrap_or_else(|_| "http://localhost:9999/proof".to_string());
        let api_key = std::env::var("RAIKO_AGENT_API_KEY")
            .ok();
        let api_key = api_key.filter(|key| !key.is_empty());

        let config = BoundlessProverConfig::from_env();

        Self {
            remote_prover_url,
            api_key,
            request_semaphore: Arc::new(Semaphore::new(config.request_concurrency_limit)),
            config,
            images_uploaded: Arc::new(tokio::sync::RwLock::new(ImagesUploaded::default())),
            auth_checked: AtomicBool::new(false),
        }
    }

    fn with_api_key(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.api_key.as_deref() {
            Some(key) if !key.is_empty() => builder.header("x-api-key", key),
            _ => builder,
        }
    }

    /// Preflight auth to surface missing or invalid API keys before large uploads.
    async fn preflight_agent_auth(&self) -> ProverResult<()> {
        if self.auth_checked.load(Ordering::Acquire) {
            return Ok(());
        }

        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let info_url = format!("{}/images", base_url);

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(self.config.http_timeout_secs))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;

        let resp = self
            .with_api_key(client.get(&info_url))
            .send()
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to query agent auth preflight: {e}"))
            })?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(ProverError::GuestError(format!(
                "Boundless agent auth preflight failed with status {}: {}",
                status, error_text
            )));
        }

        self.auth_checked.store(true, Ordering::Release);
        Ok(())
    }

    /// Check if required images are present on the agent.
    async fn verify_images_in_agent(
        &self,
        expected_batch: Option<[u32; 8]>,
        expected_agg: Option<[u32; 8]>,
    ) -> ProverResult<bool> {
        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let info_url = format!("{}/images", base_url);

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(self.config.http_timeout_secs))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;

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

        let resp_json: serde_json::Value = resp.json().await.map_err(|e| {
            ProverError::GuestError(format!("Failed to parse images response: {e}"))
        })?;

        let extract_image_id =
            |root: &serde_json::Value, key: &str| -> Option<Vec<u32>> {
                root.get(key)
                    .and_then(|entry| entry.get("image_id"))
                    .and_then(|id| serde_json::from_value::<Vec<u32>>(id.clone()).ok())
            };

        let (batch_id, agg_id) = if let Some(provers) =
            resp_json.get("provers").and_then(|v| v.as_array())
        {
            let boundless = provers.iter().find(|entry| {
                entry
                    .get("prover_type")
                    .and_then(|v| v.as_str())
                    == Some("boundless")
            });
            if let Some(boundless) = boundless {
                (
                    extract_image_id(boundless, "batch"),
                    extract_image_id(boundless, "aggregation"),
                )
            } else {
                (None, None)
            }
        } else {
            (
                extract_image_id(&resp_json, "batch"),
                extract_image_id(&resp_json, "aggregation"),
            )
        };

        let batch_ok = if let Some(expected) = expected_batch {
            batch_id
                .map(|id| id == expected.to_vec())
                .unwrap_or(false)
        } else {
            true
        };

        let agg_ok = if let Some(expected) = expected_agg {
            agg_id
                .map(|id| id == expected.to_vec())
                .unwrap_or(false)
        } else {
            true
        };

        Ok(batch_ok && agg_ok)
    }

    /// Ensure batch ELF is uploaded.
    async fn ensure_batch_uploaded(&self) -> ProverResult<()> {
        self.ensure_uploaded(ImageType::Batch).await
    }

    /// Ensure base aggregation ELF is uploaded.
    async fn ensure_base_agg_uploaded(&self) -> ProverResult<()> {
        self.ensure_uploaded(ImageType::Aggregation(AggType::Base))
            .await
    }

    /// Ensure shasta aggregation ELF is uploaded.
    async fn ensure_shasta_agg_uploaded(&self) -> ProverResult<()> {
        self.ensure_uploaded(ImageType::Aggregation(AggType::Shasta))
            .await
    }

    async fn ensure_uploaded(&self, image_type: ImageType) -> ProverResult<()> {
        let (expected_batch, expected_agg, upload_endpoint, elf_bytes, expected_image_id) =
            match image_type {
                ImageType::Batch => (
                    Some(BOUNDLESS_BATCH_ID),
                    None,
                    "batch",
                    BOUNDLESS_BATCH_ELF,
                    BOUNDLESS_BATCH_ID,
                ),
                ImageType::Aggregation(AggType::Base) => (
                    None,
                    Some(BOUNDLESS_AGGREGATION_ID),
                    "aggregation",
                    BOUNDLESS_AGGREGATION_ELF,
                    BOUNDLESS_AGGREGATION_ID,
                ),
                ImageType::Aggregation(AggType::Shasta) => (
                    None,
                    Some(BOUNDLESS_SHASTA_AGGREGATION_ID),
                    "aggregation",
                    BOUNDLESS_SHASTA_AGGREGATION_ELF,
                    BOUNDLESS_SHASTA_AGGREGATION_ID,
                ),
            };

        let mut state = self.images_uploaded.write().await;
        let already_uploaded = match image_type {
            ImageType::Batch => state.batch,
            ImageType::Aggregation(agg_type) => state.aggregation == Some(agg_type),
        };

        if already_uploaded {
            drop(state);
            if self
                .verify_images_in_agent(expected_batch, expected_agg)
                .await?
            {
                return Ok(());
            }
            state = self.images_uploaded.write().await;
        }

        match image_type {
            ImageType::Batch => tracing::info!("Uploading batch ELF image to boundless agent..."),
            ImageType::Aggregation(AggType::Base) => {
                tracing::info!("Uploading aggregation ELF image to boundless agent...")
            }
            ImageType::Aggregation(AggType::Shasta) => {
                tracing::info!("Uploading shasta aggregation ELF image to boundless agent...")
            }
        }

        self.upload_image_to_agent(upload_endpoint, elf_bytes, expected_image_id)
            .await?;

        match image_type {
            ImageType::Batch => state.batch = true,
            ImageType::Aggregation(agg_type) => state.aggregation = Some(agg_type),
        }

        Ok(())
    }

    /// Upload a single image to the agent
    async fn upload_image_to_agent(
        &self,
        image_type: &str,
        elf_bytes: &[u8],
        expected_image_id: [u32; 8],
    ) -> ProverResult<()> {
        self.preflight_agent_auth().await?;

        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let upload_url = format!("{}/upload-image/boundless/{}", base_url, image_type);

        tracing::info!(
            "Uploading {} image: {:.2} MB",
            image_type,
            elf_bytes.len() as f64 / 1_000_000.0
        );

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(120)) // Longer timeout for large uploads
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;

        let resp = client
            .post(&upload_url);
        let resp = self
            .with_api_key(resp)
            .header("Content-Type", "application/octet-stream")
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
            let error_text = resp.text().await.unwrap_or_default();
            return Err(ProverError::GuestError(format!(
                "Agent returned error status {}: {}",
                status,
                error_text
            )));
        }

        let resp_json: serde_json::Value = resp.json().await.map_err(|e| {
            ProverError::GuestError(format!("Failed to parse upload response: {e}"))
        })?;

        // Verify image_id matches what we computed
        let agent_image_id: Vec<u32> = resp_json
            .get("image_id")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .ok_or_else(|| {
                ProverError::GuestError("Missing image_id in agent response".to_string())
            })?;

        let expected_vec: Vec<u32> = expected_image_id.to_vec();

        if agent_image_id != expected_vec {
            return Err(ProverError::GuestError(format!(
                "Image ID mismatch for {}! Driver: {:?}, Agent: {:?}.",
                image_type, expected_vec, agent_image_id
            )));
        }

        let status = resp_json
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");

        tracing::info!(
            "{} image {}: {}",
            image_type,
            status,
            resp_json
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
        );

        Ok(())
    }
}

/// Poll the boundless agent status endpoint until proof is completed or failed
async fn wait_boundless_proof(
    agent_base_url: &str,
    request_id: String,
    config: &BoundlessProverConfig,
    api_key: Option<&str>,
) -> ProverResult<Vec<u8>> {
    tracing::info!("Waiting for boundless proof completion, polling agent status for request: {}", request_id);

    let max_retries = config.max_status_retries;
    let poll_interval = Duration::from_secs(config.status_poll_interval_secs);
    let max_timeout = Duration::from_secs(config.max_proof_timeout_secs);
    
    let start_time = std::time::Instant::now();
    
    // Extract base URL without /proof endpoint
    let base_url = agent_base_url.trim_end_matches("/proof");
    let status_url = format!("{}/status/{}", base_url, request_id);
    
    loop {
        // Check timeout
        if start_time.elapsed() > max_timeout {
            return Err(ProverError::GuestError(format!(
                "Boundless proof request {} timed out after {} seconds - no response from market",
                request_id,
                config.max_proof_timeout_secs
            )));
        }
        
        let mut res = None;
        for attempt in 1..=max_retries {
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(config.http_connect_timeout_secs))
                .timeout(Duration::from_secs(config.http_timeout_secs))
                .build()
                .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;

            let req = client.get(&status_url);
            let req = match api_key {
                Some(key) if !key.is_empty() => req.header("x-api-key", key),
                _ => req,
            };

            match req.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json_res) => {
                                res = Some(json_res);
                                break;
                            }
                            Err(err) => {
                                if attempt == max_retries {
                                    return Err(ProverError::GuestError(format!(
                                        "Failed to parse status response: {}", err
                                    )));
                                }
                                tracing::warn!("Attempt {}/{} failed to parse response: {}", attempt, max_retries, err);
                                tokio_async_sleep(Duration::from_secs(config.status_retry_delay_secs)).await;
                                continue;
                            }
                        }
                    } else {
                        if let Some(message) = agent_auth_error(response.status()) {
                            return Err(ProverError::GuestError(message));
                        }
                        if attempt == max_retries {
                            return Err(ProverError::GuestError(format!(
                                "Boundless agent status endpoint error after {} attempts: {}", max_retries, response.status()
                            )));
                        }
                        tracing::warn!("Attempt {}/{} - boundless agent status endpoint error: {}", attempt, max_retries, response.status());
                        tokio_async_sleep(Duration::from_secs(config.status_retry_delay_secs)).await;
                        continue;
                    }
                }
                Err(err) => {
                    if attempt == max_retries {
                        return Err(ProverError::GuestError(format!(
                            "Failed to query boundless agent status endpoint after {} attempts: {}", max_retries, err
                        )));
                    }
                    tracing::warn!("Attempt {}/{} - failed to query boundless agent status: {:?}", attempt, max_retries, err);
                    tokio_async_sleep(Duration::from_secs(config.status_retry_delay_secs)).await;
                    continue;
                }
            }
        }
        
        let res = res.ok_or_else(|| ProverError::GuestError("status result not found!".to_string()))?;
        
        let status = res.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
        let status_message = res.get("status_message").and_then(|s| s.as_str()).unwrap_or("No status message");
        
        // Use market order ID for logs when proof is in the market, otherwise use internal request_id
        let display_id = if status == "submitted" || status == "in_progress" || status == "completed" {
            res.get("market_request_id")
                .and_then(|v| v.as_str())
                .or_else(|| res.get("provider_request_id").and_then(|v| v.as_str()))
                .map(|id| format!("market order {}", id))
                .unwrap_or_else(|| format!("request {}", request_id))
        } else {
            format!("request {}", request_id)
        };
        
        match status {
            "preparing" | "submitted" | "in_progress" => {
                tracing::info!("Boundless {}: {}", display_id, status_message);
                tokio_async_sleep(poll_interval).await;
            }
            "completed" => {
                tracing::info!("Boundless {}: {}", display_id, status_message);
                // Extract proof_data
                let proof_data = res
                    .get("proof_data")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64().map(|b| b as u8))
                            .collect::<Vec<u8>>()
                    })
                    .ok_or_else(|| {
                        ProverError::GuestError("Missing proof_data in completed response".to_string())
                    })?;
                return Ok(proof_data);
            }
            "failed" => {
                // Use both status_message and error field for comprehensive error reporting
                let error_detail = res.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("No error details");
                return Err(ProverError::GuestError(format!(
                    "Boundless {} failed - {}: {}", display_id, status_message, error_detail
                )));
            }
            _ => {
                return Err(ProverError::GuestError(format!(
                    "Unknown status from boundless agent for {}: {} - {}", display_id, status, status_message
                )));
            }
        }
    }
}

impl Prover for BoundlessProver {
    async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("No need for post pacaya");
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Ensure batch ELF is uploaded before first use
        // self.ensure_batch_uploaded().await?;
        self.ensure_base_agg_uploaded().await?;

        let input_proof_hex_str = input
            .proofs
            .first()
            .and_then(|proof| proof.proof.as_ref())
            .ok_or_else(|| {
                ProverError::GuestError("Missing proof in aggregation input".to_string())
            })?;
        let input_proof_bytes = hex::decode(input_proof_hex_str.trim_start_matches("0x"))
            .map_err(|e| ProverError::GuestError(format!("Failed to decode input proof: {e}")))?;
        let input_image_id_bytes: [u8; 32] = input_proof_bytes
            .get(32..64)
            .ok_or_else(|| {
                ProverError::GuestError("Input proof too short for image_id".to_string())
            })?
            .try_into()
            .map_err(|_| {
                ProverError::GuestError("Invalid image_id bytes in input proof".to_string())
            })?;
        let input_proof_image_id = Digest::from(input_image_id_bytes);
        let agent_input = Risc0AgentAggGuestInput {
            image_id: input_proof_image_id,
            receipts: input
                .proofs
                .iter()
                .map(|p| {
                    let receipt_json = p.quote.clone().ok_or_else(|| {
                        ProverError::GuestError("Missing quote in proof for aggregation".to_string())
                    })?;
                    let receipt: ZkvmReceipt = serde_json::from_str(&receipt_json).map_err(|e| {
                        ProverError::GuestError(format!("Failed to deserialize receipt from quote: {e}"))
                    })?;
                    Ok(receipt)
                })
                .collect::<ProverResult<Vec<_>>>()?,
        };

        // Prepare the input for the agent
        let agent_input_bytes = bincode::serialize(&agent_input).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize agent input: {e}"))
        })?;

        // Compute aggregation image_id for cache key
        let agg_image_id = compute_image_id(BOUNDLESS_AGGREGATION_ELF).map_err(|e| {
            ProverError::GuestError(format!("Failed to compute image ID for BOUNDLESS_AGGREGATION_ELF: {e}"))
        })?;

        // Check cache first
        let label = cache_label(&agg_image_id, &agent_input_bytes);
        if let Some(cached_proof) = load_proof(&label) {
            tracing::info!("Using cached boundless aggregation proof");

            // Verify and return cached proof
            let journal_digest = cached_proof.journal.digest();
            let encoded_proof = verify_boundless_groth16_snark_impl(
                agg_image_id,
                cached_proof.seal.to_vec(),
                journal_digest,
            )
            .await
            .map_err(|e| {
                ProverError::GuestError(format!(
                    "Failed to verify cached aggregation proof: {e}"
                ))
            })?;
            let proof: Vec<u8> = (
                encoded_proof,
                B256::from_slice(input_proof_image_id.as_bytes()),
                B256::from_slice(agg_image_id.as_bytes()),
            )
                .abi_encode()
                .iter()
                .skip(32)
                .copied()
                .collect();

            return Ok(Proof {
                proof: Some(alloy_primitives::hex::encode_prefixed(proof)),
                input: Some(B256::from_slice(journal_digest.as_bytes())),
                quote: cached_proof.receipt,
                uuid: None,
                kzg_proof: None,
                extra_data: None,
            });
        }

        // Make a remote call to the boundless agent at localhost:9999/proof and await the response

        use reqwest::Client as HttpClient;
        use serde_json::json;

        // Compose the request payload
        // NOTE: `boundless-aggregation` guest commits `aggregation_output(program, public_inputs)`
        // via `env::commit_slice`, so the journal is the raw concatenation:
        //   32 bytes program image_id (as B256) || 32 bytes per public input (as B256)
        // (no length-prefix framing).
        let public_inputs: Vec<B256> = agent_input
            .receipts
            .iter()
            .map(|receipt| B256::from_slice(&receipt.journal.bytes[4..]))
            .collect();
        let batch_image_words: [u32; 8] = input_proof_image_id
            .as_words()
            .try_into()
            .expect("image_id should have 8 words");
        let program =
            B256::from(raiko_lib::protocol_instance::words_to_bytes_le(&batch_image_words));
        let expected_output =
            raiko_lib::protocol_instance::aggregation_output(program, public_inputs);

        let payload = json!({
            "prover_type": "boundless",
            "input": agent_input_bytes,
            "proof_type": "Aggregate",
            "output": expected_output,
        });

        // Acquire semaphore permit to limit concurrent HTTP requests
        let _permit = self.request_semaphore.acquire().await.map_err(|e| {
            ProverError::GuestError(format!("Failed to acquire request semaphore: {e}"))
        })?;

        // Send the request to the agent and await the response
        let client = HttpClient::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(self.config.http_timeout_secs))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;
        let resp = client
            .post(&self.remote_prover_url);
        let resp = self
            .with_api_key(resp)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to send request to agent: {e}"))
            })?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            return Err(ProverError::GuestError(format!(
                "Agent returned error status: {}",
                resp.status()
            )));
        }

        // Parse the response
        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to parse agent response: {e}")))?;

        // Extract request_id from initial response for polling
        let request_id = resp_json.get("request_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProverError::GuestError("Missing request_id in agent response".to_string()))?;

        // Poll until completion
        let agent_proof_bytes = wait_boundless_proof(
            &self.remote_prover_url,
            request_id.to_string(),
            &self.config,
            self.api_key.as_deref(),
        )
        .await?;

        let agent_proof: Risc0AgentResponse =
            bincode::deserialize(&agent_proof_bytes).map_err(|e| {
                ProverError::GuestError(format!("Failed to deserialize output file: {e}"))
            })?;

        // Save to cache after receiving from agent
        save_proof(&label, &agent_proof);

        let journal_digest = agent_proof.journal.digest();
        let encoded_proof = verify_boundless_groth16_snark_impl(
            agg_image_id,
            agent_proof.seal.to_vec(),
            journal_digest,
        )
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to verify groth16 snark: {e}")))?;
        let proof: Vec<u8> = (
            encoded_proof,
            B256::from_slice(input_proof_image_id.as_bytes()),
            B256::from_slice(agg_image_id.as_bytes()),
        )
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect();

        Ok(Proof {
            proof: Some(alloy_primitives::hex::encode_prefixed(proof)),
            input: Some(B256::from_slice(journal_digest.as_bytes())),
            quote: agent_proof.receipt,
            uuid: None,
            kzg_proof: None,
            extra_data: None,
        })
    }

    async fn cancel(&self, _key: ProofKey, _id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        todo!()
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Ensure batch + shasta aggregation ELF are uploaded before first use
        // self.ensure_batch_uploaded().await?;
        self.ensure_shasta_agg_uploaded().await?;

        // Deserialize receipts and collect block inputs
        let receipts: Vec<ZkvmReceipt> = input
            .proofs
            .iter()
            .map(|p| {
                let receipt_json = p.quote.clone().ok_or_else(|| {
                    ProverError::GuestError("Missing quote in proof for shasta aggregation".into())
                })?;
                serde_json::from_str(&receipt_json).map_err(|e| {
                    ProverError::GuestError(format!(
                        "Failed to deserialize receipt from quote: {e}"
                    ))
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;

        let input_proof_hex_str = input
            .proofs
            .first()
            .and_then(|proof| proof.proof.as_ref())
            .ok_or_else(|| {
                ProverError::GuestError("Missing proof in shasta aggregation input".to_string())
            })?;
        let input_proof_bytes = hex::decode(input_proof_hex_str.trim_start_matches("0x"))
            .map_err(|e| ProverError::GuestError(format!("Failed to decode input proof: {e}")))?;
        let input_image_id_bytes: [u8; 32] = input_proof_bytes
            .get(32..64)
            .ok_or_else(|| {
                ProverError::GuestError("Input proof too short for image_id".to_string())
            })?
            .try_into()
            .map_err(|_| {
                ProverError::GuestError("Invalid image_id bytes in input proof".to_string())
            })?;
        let input_proof_image_id = Digest::from(input_image_id_bytes);

        let proof_carry_data_vec: Vec<ProofCarryData> = input
            .proofs
            .iter()
            .map(|p| {
                p.extra_data.clone().ok_or_else(|| {
                    ProverError::GuestError(
                        "Missing extra_data (proof carry data) in proof for shasta aggregation"
                            .to_string(),
                    )
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;
        validate_shasta_inputs(&input.proofs, &proof_carry_data_vec)?;

        let agent_input = BoundlessShastaAggregationGuestInput {
            image_id: input_proof_image_id,
            receipts: receipts.clone(),
            proof_carry_data_vec: proof_carry_data_vec.clone(),
        };

        // Bincode-encode for the boundless guest which reads a framed buffer.
        let agent_input_bytes = bincode::serialize(&agent_input).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize shasta agent input: {e}"))
        })?;

        // Cache key must also reflect the receipts that will be re-verified.
        let receipts_bytes = bincode::serialize(&receipts).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize shasta receipts: {e}"))
        })?;

        let agg_image_id = compute_image_id(BOUNDLESS_SHASTA_AGGREGATION_ELF).map_err(|e| {
            ProverError::GuestError(format!(
                "Failed to compute image ID for BOUNDLESS_SHASTA_AGGREGATION_ELF: {e}"
            ))
        })?;

        // Check cache first
        let mut cache_key = agent_input_bytes.clone();
        cache_key.extend_from_slice(&receipts_bytes);
        let label = cache_label(&agg_image_id, &cache_key);
        if let Some(cached_proof) = load_proof(&label) {
            tracing::info!("Using cached boundless shasta aggregation proof");

            let journal_digest = cached_proof.journal.digest();
            let encoded_proof = verify_boundless_groth16_snark_impl(
                agg_image_id,
                cached_proof.seal.to_vec(),
                journal_digest,
            )
            .await
            .map_err(|e| {
                ProverError::GuestError(format!(
                    "Failed to verify cached shasta aggregation proof: {e}"
                ))
            })?;
            let proof: Vec<u8> = (
                encoded_proof,
                B256::from_slice(input_proof_image_id.as_bytes()),
                B256::from_slice(agg_image_id.as_bytes()),
            )
                .abi_encode()
                .iter()
                .skip(32)
                .copied()
                .collect();

            return Ok(Proof {
                proof: Some(alloy_primitives::hex::encode_prefixed(proof)),
                input: Some(B256::from_slice(journal_digest.as_bytes())),
                quote: cached_proof.receipt,
                uuid: None,
                kzg_proof: None,
                extra_data: None,
            });
        }

        // Prepare payload for agent
        let image_words: [u32; 8] = input_proof_image_id
            .as_words()
            .try_into()
            .expect("image_id should have 8 words");
        let sub_image_id = B256::from(raiko_lib::protocol_instance::words_to_bytes_le(&image_words));
        let expected_output_hash =
            raiko_lib::protocol_instance::shasta_aggregation_hash_for_zk(
                sub_image_id,
                &proof_carry_data_vec,
            )
            .ok_or_else(|| {
                ProverError::GuestError(
                    "invalid/mismatched shasta proof carry data for aggregation".to_string(),
                )
            })?;
        // NOTE: `boundless-shasta-aggregation` guest uses `env::commit_slice(bytes32)`, so the
        // journal is exactly 32 bytes (no length prefix framing).
        let expected_output_bytes: &[u8; 32] = expected_output_hash.as_ref();
        let expected_output: Vec<u8> = expected_output_bytes.to_vec();

        let payload = serde_json::json!({
            "prover_type": "boundless",
            "input": agent_input_bytes,
            "proof_type": "Aggregate",
            "output": expected_output,
        });

        // Acquire semaphore permit to limit concurrent HTTP requests
        let _permit = self.request_semaphore.acquire().await.map_err(|e| {
            ProverError::GuestError(format!("Failed to acquire request semaphore: {e}"))
        })?;

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(self.config.http_timeout_secs))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;
        let resp = client
            .post(&self.remote_prover_url);
        let resp = self
            .with_api_key(resp)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to send shasta aggregation request: {e}"))
            })?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            return Err(ProverError::GuestError(format!(
                "Agent returned error status: {}",
                resp.status()
            )));
        }

        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to parse agent response: {e}")))?;

        let request_id = resp_json
            .get("request_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProverError::GuestError("Missing request_id in agent response".to_string())
            })?;

        // Poll until completion
        let agent_proof_bytes =
            wait_boundless_proof(
                &self.remote_prover_url,
                request_id.to_string(),
                &self.config,
                self.api_key.as_deref(),
            )
                .await?;

        let agent_proof: Risc0AgentResponse =
            bincode::deserialize(&agent_proof_bytes).map_err(|e| {
                ProverError::GuestError(format!(
                    "Failed to deserialize shasta aggregation output file: {e}"
                ))
            })?;

        // Save to cache after receiving from agent
        save_proof(&label, &agent_proof);

        let journal_digest = agent_proof.journal.digest();
        let encoded_proof = verify_boundless_groth16_snark_impl(
            agg_image_id,
            agent_proof.seal.to_vec(),
            journal_digest,
        )
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to verify groth16 snark: {e}")))?;
        let proof_bytes: Vec<u8> = (
            encoded_proof,
            B256::from_slice(input_proof_image_id.as_bytes()),
            B256::from_slice(agg_image_id.as_bytes()),
        )
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect();

        Ok(Proof {
            proof: Some(alloy_primitives::hex::encode_prefixed(proof_bytes)),
            input: Some(B256::from_slice(journal_digest.as_bytes())),
            quote: agent_proof.receipt,
            uuid: None,
            kzg_proof: None,
            extra_data: None,
        })
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Ensure batch + base aggregation ELF are uploaded before first use
        self.ensure_batch_uploaded().await?;

        // Serialize the input using bincode
        let input_bytes = bincode::serialize(&input).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize input with bincode: {e}"))
        })?;

        // Compute image_id for cache key
        let image_id = compute_image_id(BOUNDLESS_BATCH_ELF).map_err(|e| {
            ProverError::GuestError(format!("Failed to compute image ID for BOUNDLESS_BATCH_ELF: {e}"))
        })?;

        // Check cache first
        let label = cache_label(&image_id, &input_bytes);
        if let Some(cached_proof) = load_proof(&label) {
            tracing::info!("Using cached boundless batch proof for batch_id: {}", input.taiko.batch_id);

            // Verify and return cached proof
            let journal_digest = cached_proof.journal.digest();
            let encoded_proof = verify_boundless_groth16_snark_impl(
                image_id,
                cached_proof.seal.to_vec(),
                journal_digest,
            ).await.map_err(|e| ProverError::GuestError(format!("Failed to verify cached proof: {e}")))?;

            let proof_bytes: Vec<u8> = (encoded_proof, B256::from_slice(image_id.as_bytes()))
                .abi_encode()
                .iter()
                .skip(32)
                .copied()
                .collect();

            return Ok(Risc0Response {
                proof: alloy_primitives::hex::encode_prefixed(proof_bytes),
                receipt: cached_proof.receipt.unwrap_or_default(),
                uuid: "cached".to_string(),
                input: output.hash,
            }.into());
        }


        // Log input information, especially the batch_id
        tracing::info!(
            "Risc0 Boundless batch prover starting for batch_id: {}, input size: {}",
            input.taiko.batch_id,
            input_bytes.len()
        );

        // Construct the request payload for the agent
        // Send the expected output hash in the same Vec<u8> journal format (len prefix + bytes)
        let output_hash_bytes: &[u8] = output.hash.as_ref();
        let mut output_hash_vec = Vec::with_capacity(4 + output_hash_bytes.len());
        output_hash_vec.extend_from_slice(&(output_hash_bytes.len() as u32).to_le_bytes());
        output_hash_vec.extend_from_slice(output_hash_bytes);

        let payload = serde_json::json!({
            "prover_type": "boundless",
            "input": input_bytes,
            "proof_type": "Batch",
            "output": output_hash_vec,
        });

        // Acquire semaphore permit to limit concurrent HTTP requests
        let _permit = self.request_semaphore.acquire().await.map_err(|e| {
            ProverError::GuestError(format!("Failed to acquire request semaphore: {e}"))
        })?;

        // Send the request to the local agent and handle the response
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(self.config.http_timeout_secs))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;
        let resp = client
            .post(&self.remote_prover_url);
        let resp = self
            .with_api_key(resp)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to send request to agent: {e}"))
            })?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            return Err(ProverError::GuestError(format!(
                "Agent {} returned error status: {}",
                self.remote_prover_url,
                resp.status()
            )));
        }

        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to parse agent response: {e}")))?;

        // Extract request_id from initial response for polling
        let request_id = resp_json.get("request_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProverError::GuestError("Missing request_id in agent response".to_string()))?;

        // Poll until completion
        let agent_proof_bytes = wait_boundless_proof(
            &self.remote_prover_url,
            request_id.to_string(),
            &self.config,
            self.api_key.as_deref(),
        )
        .await?;

        let agent_proof: Risc0AgentResponse =
            bincode::deserialize(&agent_proof_bytes).map_err(|e| {
                ProverError::GuestError(format!("Failed to deserialize output file: {e}"))
            })?;

        // Save to cache after receiving from agent
        save_proof(&label, &agent_proof);

        let journal_digest = agent_proof.journal.digest();
        let encoded_proof = verify_boundless_groth16_snark_impl(
            image_id,
            agent_proof.seal.to_vec(),
            journal_digest,
        )
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to verify groth16 snark: {e}")))?;
        let proof_bytes: Vec<u8> = (encoded_proof, B256::from_slice(image_id.as_bytes()))
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect();
        Ok(Risc0Response {
            proof: alloy_primitives::hex::encode_prefixed(proof_bytes),
            receipt: agent_proof.receipt.unwrap(),
            uuid: "".to_string(), // can be request tx hash
            input: output.hash,
        }
        .into())
    }

    fn proof_type(&self) -> ProofType {
        ProofType::Risc0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_logger;
    use raiko_lib::input::GuestBatchOutput;

    #[ignore = "reason: no need to run in CI"]
    #[tokio::test]
    async fn test_run_prover() {
        // init log
        env_logger::init();

        let input_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/input-1306738.json").unwrap();
        let output_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/output-1306738.json").unwrap();
        let input: GuestBatchInput = serde_json::from_slice(&input_file).unwrap();
        let output: GuestBatchOutput = serde_json::from_slice(&output_file).unwrap();
        let config = ProverConfig::default();
        let proof = BoundlessProver::new()
            .batch_run(input, &output, &config, None)
            .await
            .unwrap();
        println!("proof: {:?}", proof);

        // Save the boundless_receipt as JSON to a file for later deserialization.
        // The file name can be based on the request_id or image_id for uniqueness.
        let receipt_json = serde_json::to_string_pretty(&proof).unwrap();
        let file_name = format!("../../../boundless_receipt_test.json");
        if let Err(e) = std::fs::write(&file_name, receipt_json) {
            tracing::warn!(
                "Failed to write boundless_receipt to file {}: {}",
                file_name,
                e
            );
        } else {
            tracing::info!("Saved boundless_receipt to file: {}", file_name);
        }
    }

    #[ignore = "not needed in CI"]
    #[tokio::test]
    async fn test_transfer_input_output() {
        // init log
        env_logger::init();

        let input_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/input-1306738.json").unwrap();
        let output_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/output-1306738.json").unwrap();
        let input: GuestBatchInput = serde_json::from_slice(&input_file).unwrap();
        let output: GuestBatchOutput = serde_json::from_slice(&output_file).unwrap();

        let input_bytes = bincode::serialize(&input).unwrap();
        let output_bytes = bincode::serialize(&output).unwrap();
        // println!("input_bytes: {:?}", input_bytes);
        // println!("output_bytes: {:?}", output_bytes);

        //save to file
        let input_file_name = format!("../../../input-1306738.bin");
        let output_file_name = format!("../../../output-1306738.bin");
        std::fs::write(&input_file_name, input_bytes).unwrap();
        std::fs::write(&output_file_name, output_bytes).unwrap();
        println!("Saved input to file: {}", input_file_name);
        println!("Saved output to file: {}", output_file_name);

        // deserialize from data & check equality
        let input_bytes = std::fs::read(&input_file_name).unwrap();
        let output_bytes = std::fs::read(&output_file_name).unwrap();
        let _input_deserialized: GuestBatchInput =
            bincode::deserialize(&input_bytes).expect("Failed to deserialize input");
        let _output_deserialized: GuestBatchOutput =
            bincode::deserialize(&output_bytes).expect("Failed to deserialize output");
    }

    #[ignore = "not needed in CI"]
    #[tokio::test]
    async fn test_run_prover_with_seal() {
        env_logger::init();

        use crate::RISC0_BATCH_ELF;
        let seal = alloy_primitives::hex::decode("0x9f39696c021c04f95caa9962aa0022f0eae58f1cd7e13ccf553a152a3d0e91443d0aab4f25a24e93423c51f1ae46e604e20a360cfe2376e7270a10d1f4a9e665adcc91e713155b2e45e05edb00c7f044ab827a425cac6d0c932e3e14aeddf79200a8fe7711ad2207298cf2004c5dffc5956e9b30d6b98e9e2533b1e6944671f35dacf85823bb4fd3e0dd14a0000bc3304338f844b11095d1dbfedf3e90074bf7c666ed531dd4676c51fdf0111529d5c40719d36ba8ba11db8542fff1bca90c24255c515f1b6e32a396bf2bdb40ad165f949f1d46c533266a666e3b6684ddbbbc8c4ce5c1051676d81b1addd377e8b9665912d32347aac64c1a9b38faaab63ceeb1dcc67c").unwrap();
        let image_id = compute_image_id(RISC0_BATCH_ELF).unwrap();
        let journal = alloy_primitives::hex::decode(
            "0x20000000b0284764aae7f327410ae1355bef23dcccd0f9c723724d6638a8edde86091d65",
        )
        .unwrap();
        let journal_digest = journal.digest();
        let encoded_proof =
            verify_boundless_groth16_snark_impl(image_id, seal, journal_digest.into())
                .await
                .unwrap();
        println!("encoded_proof: {:?}", encoded_proof);
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
        let proof: Proof =
            serde_json::from_str(proof_json).expect("Failed to deserialize proof JSON");
        println!("Deserialized receipt: {:#?}", proof);
    }

    #[ignore = "not needed in CI"]
    #[test]
    fn test_deserialize_zkvm_receipt() {
        let file_name = format!("./boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Proof = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let zkvm_receipt: ZkvmReceipt = serde_json::from_str(&proof.quote.unwrap()).unwrap();
        println!("Deserialized zkvm receipt: {:#?}", zkvm_receipt);
    }

    #[ignore = "reason: no need to run in CI"]
    #[tokio::test]
    async fn test_run_proof_aggregation() {
        env_logger::init();

        let file_name = format!("../../../boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Proof = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let input: AggregationGuestInput = AggregationGuestInput {
            proofs: vec![proof],
        };
        let output: AggregationGuestOutput = AggregationGuestOutput { hash: B256::ZERO };
        let config = ProverConfig::default();
        let proof = BoundlessProver::new()
            .aggregate(input, &output, &config, None)
            .await
            .unwrap();
        println!("proof: {:?}", proof);
    }
}
