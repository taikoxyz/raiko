#![cfg(feature = "enable")]

use alloy_primitives::{hex, B256, U256};
use alloy_sol_types::SolValue;
use once_cell::sync::Lazy;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput, ShastaBrevisAggregationGuestInput,
        ZkAggregationGuestInput,
    },
    libhash::hash_shasta_subproof_input,
    proof_type::ProofType,
    protocol_instance::validate_shasta_proof_carry_data_vec,
    prover::{
        IdStore, IdWrite, Proof, ProofCarryData, ProofKey, Prover, ProverConfig, ProverError,
        ProverResult,
    },
};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf, time::Duration};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrevisPicoProofBundle {
    pub riscv_vkey: [u8; 32],
    pub public_values: Vec<u8>,
    pub proof: [U256; 8],
    pub pico_proof: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BrevisPicoAggregationInput {
    guest_input: Vec<u8>,
    pico_proofs: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BrevisPicoParam {
    pub agent_url: Option<String>,
    pub api_key: Option<String>,
    pub batch_elf_path: Option<String>,
    pub aggregation_elf_path: Option<String>,
    pub shasta_aggregation_elf_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BrevisPicoProverConfig {
    pub status_poll_interval_secs: u64,
    pub max_proof_timeout_secs: u64,
    pub max_status_retries: u32,
    pub status_retry_delay_secs: u64,
    pub http_connect_timeout_secs: u64,
    pub http_timeout_secs: u64,
}

impl Default for BrevisPicoProverConfig {
    fn default() -> Self {
        Self {
            status_poll_interval_secs: 15,
            max_proof_timeout_secs: 3600,
            max_status_retries: 8,
            status_retry_delay_secs: 15,
            http_connect_timeout_secs: 10,
            http_timeout_secs: 60,
        }
    }
}

impl BrevisPicoProverConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            status_poll_interval_secs: env::var("BREVIS_STATUS_POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.status_poll_interval_secs),
            max_proof_timeout_secs: env::var("BREVIS_MAX_PROOF_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.max_proof_timeout_secs),
            max_status_retries: env::var("BREVIS_MAX_STATUS_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.max_status_retries),
            status_retry_delay_secs: env::var("BREVIS_STATUS_RETRY_DELAY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.status_retry_delay_secs),
            http_connect_timeout_secs: env::var("BREVIS_HTTP_CONNECT_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.http_connect_timeout_secs),
            http_timeout_secs: env::var("BREVIS_HTTP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.http_timeout_secs),
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct ImagesUploaded {
    batch: bool,
    aggregation: Option<AggType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AggType {
    Base,
    Shasta,
}

static IMAGES_UPLOADED: Lazy<RwLock<ImagesUploaded>> =
    Lazy::new(|| RwLock::new(ImagesUploaded::default()));

#[derive(Clone, Copy)]
enum ImageType {
    Batch,
    Aggregation(AggType),
}

impl ImageType {
    fn as_str(&self) -> &'static str {
        match self {
            ImageType::Batch => "batch",
            ImageType::Aggregation(_) => "aggregation",
        }
    }
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

struct BrevisPicoClient {
    remote_prover_url: String,
    api_key: Option<String>,
    config: BrevisPicoProverConfig,
    params: BrevisPicoParam,
}

impl BrevisPicoClient {
    fn from_config(config: &ProverConfig) -> ProverResult<Self> {
        let params = match config.get("brevis").or_else(|| config.get("brevis_pico")) {
            Some(value) => serde_json::from_value(value.clone()).map_err(ProverError::Param)?,
            None => BrevisPicoParam::default(),
        };

        let remote_prover_url = params
            .agent_url
            .clone()
            .or_else(|| env::var("RAIKO_AGENT_URL").ok())
            .unwrap_or_else(|| "http://localhost:9999/proof".to_string());

        let api_key = params
            .api_key
            .clone()
            .or_else(|| env::var("RAIKO_AGENT_API_KEY").ok())
            .filter(|key| !key.is_empty());

        Ok(Self {
            remote_prover_url,
            api_key,
            config: BrevisPicoProverConfig::from_env(),
            params,
        })
    }

    fn resolve_elf_path(&self, image_type: ImageType) -> ProverResult<PathBuf> {
        let (env_var, path, label) = match image_type {
            ImageType::Batch => (
                "BREVIS_BATCH_ELF",
                self.params
                    .batch_elf_path
                    .clone()
                    .or_else(|| env::var("BREVIS_BATCH_ELF").ok()),
                "batch_elf_path",
            ),
            ImageType::Aggregation(AggType::Base) => (
                "BREVIS_AGG_ELF",
                self.params
                    .aggregation_elf_path
                    .clone()
                    .or_else(|| env::var("BREVIS_AGG_ELF").ok()),
                "aggregation_elf_path",
            ),
            ImageType::Aggregation(AggType::Shasta) => (
                "BREVIS_SHASTA_AGG_ELF",
                self.params
                    .shasta_aggregation_elf_path
                    .clone()
                    .or_else(|| env::var("BREVIS_SHASTA_AGG_ELF").ok()),
                "shasta_aggregation_elf_path",
            ),
        };

        let path = path
        .ok_or_else(|| {
            ProverError::GuestError(format!(
                "Missing Brevis {} ELF path; set brevis.{} (or brevis_pico.{}) or {}",
                image_type.as_str(),
                label,
                label,
                env_var
            ))
        })?;

        Ok(PathBuf::from(path))
    }

    async fn ensure_image_uploaded(&self, image_type: ImageType) -> ProverResult<()> {
        let already_uploaded = {
            let state = IMAGES_UPLOADED.read().await;
            match image_type {
                ImageType::Batch => state.batch,
                ImageType::Aggregation(agg_type) => state.aggregation == Some(agg_type),
            }
        };

        if already_uploaded {
            return Ok(());
        }

        let elf_path = self.resolve_elf_path(image_type)?;
        let elf_bytes = fs::read(&elf_path)?;

        tracing::info!(
            "Uploading Brevis {} ELF from {}",
            image_type.as_str(),
            elf_path.display()
        );

        self.upload_image_to_agent(image_type, &elf_bytes).await?;

        let mut state = IMAGES_UPLOADED.write().await;
        match image_type {
            ImageType::Batch => state.batch = true,
            ImageType::Aggregation(agg_type) => state.aggregation = Some(agg_type),
        }

        Ok(())
    }

    async fn upload_image_to_agent(
        &self,
        image_type: ImageType,
        elf_bytes: &[u8],
    ) -> ProverResult<()> {
        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let upload_url = format!(
            "{}/upload-image/brevis/{}",
            base_url,
            image_type.as_str()
        );

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;

        let req = client
            .post(&upload_url)
            .header("Content-Type", "application/octet-stream")
            .body(elf_bytes.to_vec());
        let req = match self.api_key.as_deref() {
            Some(key) if !key.is_empty() => req.header("x-api-key", key),
            _ => req,
        };

        let resp = req
            .send()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to upload image: {e}")))?;

        if !resp.status().is_success() {
            if let Some(message) = agent_auth_error(resp.status()) {
                return Err(ProverError::GuestError(message));
            }
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(ProverError::GuestError(format!(
                "Agent returned error status {}: {}",
                status, error_text
            )));
        }

        Ok(())
    }

    async fn submit_proof(&self, proof_type: &str, input: Vec<u8>) -> ProverResult<Vec<u8>> {
        let payload = serde_json::json!({
            "prover_type": "brevis",
            "input": input,
            "output": Vec::<u8>::new(),
            "proof_type": proof_type,
        });

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
            .timeout(Duration::from_secs(self.config.http_timeout_secs))
            .build()
            .map_err(|e| ProverError::GuestError(format!("Failed to build HTTP client: {e}")))?;

        let req = client.post(&self.remote_prover_url);
        let req = match self.api_key.as_deref() {
            Some(key) if !key.is_empty() => req.header("x-api-key", key),
            _ => req,
        };

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

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

        let request_id = resp_json
            .get("request_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProverError::GuestError("Missing request_id in agent response".to_string()))?;

        self.wait_proof(request_id.to_string()).await
    }

    async fn wait_proof(&self, request_id: String) -> ProverResult<Vec<u8>> {
        tracing::info!(
            "Waiting for Brevis Pico proof completion, polling agent status for request: {}",
            request_id
        );

        let max_retries = self.config.max_status_retries;
        let poll_interval = Duration::from_secs(self.config.status_poll_interval_secs);
        let max_timeout = Duration::from_secs(self.config.max_proof_timeout_secs);
        let start_time = std::time::Instant::now();

        let base_url = self.remote_prover_url.trim_end_matches("/proof");
        let status_url = format!("{}/status/{}", base_url, request_id);

        loop {
            if start_time.elapsed() > max_timeout {
                return Err(ProverError::GuestError(format!(
                    "Brevis Pico proof request {} timed out after {} seconds",
                    request_id, self.config.max_proof_timeout_secs
                )));
            }

            let mut res = None;
            for attempt in 1..=max_retries {
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(self.config.http_connect_timeout_secs))
                    .timeout(Duration::from_secs(self.config.http_timeout_secs))
                    .build()
                    .map_err(|e| {
                        ProverError::GuestError(format!("Failed to build HTTP client: {e}"))
                    })?;

                let req = client.get(&status_url);
                let req = match self.api_key.as_deref() {
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
                                            "Failed to parse status response: {}",
                                            err
                                        )));
                                    }
                                    tracing::warn!(
                                        "Attempt {}/{} failed to parse response: {}",
                                        attempt,
                                        max_retries,
                                        err
                                    );
                                    tokio::time::sleep(Duration::from_secs(
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
                                    "Brevis Pico agent status endpoint error after {} attempts: {}",
                                    max_retries,
                                    response.status()
                                )));
                            }
                            tracing::warn!(
                                "Attempt {}/{} - brevis pico agent status endpoint error: {}",
                                attempt,
                                max_retries,
                                response.status()
                            );
                            tokio::time::sleep(Duration::from_secs(
                                self.config.status_retry_delay_secs,
                            ))
                            .await;
                            continue;
                        }
                    }
                    Err(err) => {
                        if attempt == max_retries {
                            return Err(ProverError::GuestError(format!(
                                "Failed to query brevis pico agent status endpoint after {} attempts: {}",
                                max_retries,
                                err
                            )));
                        }
                        tracing::warn!(
                            "Attempt {}/{} - failed to query brevis pico agent status: {:?}",
                            attempt,
                            max_retries,
                            err
                        );
                        tokio::time::sleep(Duration::from_secs(
                            self.config.status_retry_delay_secs,
                        ))
                        .await;
                        continue;
                    }
                }
            }

            let res = res.ok_or_else(|| ProverError::GuestError("status result not found".to_string()))?;

            let status = res.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
            let status_message = res
                .get("status_message")
                .and_then(|s| s.as_str())
                .unwrap_or("No status message");

            match status {
                "preparing" | "submitted" | "in_progress" => {
                    tracing::info!("Brevis Pico request {}: {}", request_id, status_message);
                    tokio::time::sleep(poll_interval).await;
                }
                "completed" => {
                    tracing::info!("Brevis Pico request {}: {}", request_id, status_message);
                    let proof_data = res
                        .get("proof_data")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_u64().map(|b| b as u8))
                                .collect::<Vec<u8>>()
                        })
                        .ok_or_else(|| {
                            ProverError::GuestError(
                                "Missing proof_data in completed response".to_string(),
                            )
                        })?;
                    return Ok(proof_data);
                }
                "failed" => {
                    let error_detail = res
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("No error details");
                    return Err(ProverError::GuestError(format!(
                        "Brevis Pico request {} failed - {}: {}",
                        request_id, status_message, error_detail
                    )));
                }
                _ => {
                    return Err(ProverError::GuestError(format!(
                        "Unknown status from brevis pico agent for request {}: {} - {}",
                        request_id, status, status_message
                    )));
                }
            }
        }
    }
}

pub struct BrevisPicoProver;

impl BrevisPicoProver {
    fn bundle_to_proof(bundle: BrevisPicoProofBundle) -> Proof {
        let riscv_vkey = B256::from_slice(&bundle.riscv_vkey);
        let public_values = bundle.public_values.clone();
        let encoded = (riscv_vkey, public_values.clone(), bundle.proof).abi_encode();

        let input = if public_values.len() == 32 {
            Some(B256::from_slice(&public_values))
        } else {
            None
        };
        let quote = (!bundle.pico_proof.is_empty())
            .then(|| hex::encode_prefixed(bundle.pico_proof));

        Proof {
            proof: Some(hex::encode_prefixed(encoded)),
            input,
            quote,
            uuid: Some(hex::encode_prefixed(bundle.riscv_vkey)),
            kzg_proof: None,
            extra_data: None,
        }
    }
}

impl Prover for BrevisPicoProver {
    async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("no block run after pacaya fork")
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let client = BrevisPicoClient::from_config(config)?;
        client.ensure_image_uploaded(ImageType::Batch).await?;

        let input_bytes = bincode::serialize(&input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;

        let proof_bytes = client.submit_proof("Batch", input_bytes).await?;
        let bundle: BrevisPicoProofBundle = bincode::deserialize(&proof_bytes)
            .map_err(|e| ProverError::GuestError(format!("Failed to decode proof bundle: {e}")))?;

        Ok(Self::bundle_to_proof(bundle))
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let client = BrevisPicoClient::from_config(config)?;
        client
            .ensure_image_uploaded(ImageType::Aggregation(AggType::Base))
            .await?;

        let (image_id, block_inputs) = parse_aggregation_inputs(&input)?;
        let pico_proofs = collect_pico_proofs(&input.proofs)?;
        let agg_input = ZkAggregationGuestInput {
            image_id,
            block_inputs,
        };
        let guest_input = bincode::serialize(&agg_input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
        let input_bytes = bincode::serialize(&BrevisPicoAggregationInput {
            guest_input,
            pico_proofs,
        })
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;

        let proof_bytes = client.submit_proof("Aggregate", input_bytes).await?;
        let bundle: BrevisPicoProofBundle = bincode::deserialize(&proof_bytes)
            .map_err(|e| ProverError::GuestError(format!("Failed to decode proof bundle: {e}")))?;

        Ok(Self::bundle_to_proof(bundle))
    }

    async fn proposal_run(
        &self,
        _input: GuestBatchInput,
        _output: &GuestBatchOutput,
        _config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        Err(ProverError::GuestError(
            "Brevis Pico shasta proposals are not supported".to_string(),
        ))
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let client = BrevisPicoClient::from_config(config)?;
        client
            .ensure_image_uploaded(ImageType::Aggregation(AggType::Shasta))
            .await?;

        let (image_id, block_inputs) = parse_aggregation_inputs(&AggregationGuestInput {
            proofs: input.proofs.clone(),
        })?;
        let pico_proofs = collect_pico_proofs(&input.proofs)?;
        let proof_carry_data_vec: Vec<ProofCarryData> = input
            .proofs
            .iter()
            .map(|proof| {
                proof
                    .extra_data
                    .clone()
                    .ok_or_else(|| {
                        ProverError::GuestError(
                            "Missing extra_data (proof carry data) in shasta proof".to_string(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        validate_shasta_inputs(&input.proofs, &proof_carry_data_vec)?;

        let shasta_input = ShastaBrevisAggregationGuestInput {
            image_id,
            block_inputs,
            proof_carry_data_vec,
        };
        let guest_input = bincode::serialize(&shasta_input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
        let input_bytes = bincode::serialize(&BrevisPicoAggregationInput {
            guest_input,
            pico_proofs,
        })
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;

        let proof_bytes = client.submit_proof("Aggregate", input_bytes).await?;
        let bundle: BrevisPicoProofBundle = bincode::deserialize(&proof_bytes)
            .map_err(|e| ProverError::GuestError(format!("Failed to decode proof bundle: {e}")))?;

        Ok(Self::bundle_to_proof(bundle))
    }

    async fn cancel(&self, _proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    fn proof_type(&self) -> ProofType {
        ProofType::BrevisPico
    }
}

fn parse_aggregation_inputs(
    input: &AggregationGuestInput,
) -> ProverResult<([u32; 8], Vec<B256>)> {
    let first = input.proofs.first().ok_or_else(|| {
        ProverError::GuestError("empty aggregation request for brevis pico".to_string())
    })?;
    let image_id = parse_proof_vkey(first)?;

    let mut block_inputs = Vec::with_capacity(input.proofs.len());
    for (idx, proof) in input.proofs.iter().enumerate() {
        let proof_image_id = parse_proof_vkey(proof)?;
        if proof_image_id != image_id {
            return Err(ProverError::GuestError(format!(
                "brevis pico aggregation input has mismatched vkey at index {idx}"
            )));
        }
        block_inputs.push(parse_proof_public_input(proof)?);
    }

    Ok((image_id, block_inputs))
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
        let proof_input = parse_proof_public_input(proof)?;
        let expected = hash_shasta_subproof_input(carry);
        if proof_input != expected {
            return Err(ProverError::GuestError(format!(
                "shasta proof input mismatch at index {idx}"
            )));
        }
    }

    Ok(())
}

fn parse_proof_vkey(proof: &Proof) -> ProverResult<[u32; 8]> {
    let uuid = proof.uuid.as_deref().ok_or_else(|| {
        ProverError::GuestError("missing riscv_vkey in brevis proof".to_string())
    })?;
    let vkey_bytes = decode_hex_bytes("riscv_vkey", uuid)?;
    let vkey_bytes = bytes32_from_vec("riscv_vkey", vkey_bytes)?;
    Ok(bytes32_to_words_le(&vkey_bytes))
}

fn parse_proof_public_input(proof: &Proof) -> ProverResult<B256> {
    proof.input.ok_or_else(|| {
        ProverError::GuestError("missing public input for brevis proof".to_string())
    })
}

fn collect_pico_proofs(proofs: &[Proof]) -> ProverResult<Vec<Vec<u8>>> {
    proofs.iter().map(parse_pico_proof_bytes).collect()
}

fn parse_pico_proof_bytes(proof: &Proof) -> ProverResult<Vec<u8>> {
    let quote = proof.quote.as_deref().ok_or_else(|| {
        ProverError::GuestError("missing pico proof for brevis aggregation".to_string())
    })?;
    let bytes = decode_hex_bytes("pico_proof", quote)?;
    if bytes.is_empty() {
        return Err(ProverError::GuestError(
            "empty pico proof for brevis aggregation".to_string(),
        ));
    }
    Ok(bytes)
}

fn decode_hex_bytes(label: &str, value: &str) -> ProverResult<Vec<u8>> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    hex::decode(value).map_err(|e| {
        ProverError::GuestError(format!("invalid hex for {label}: {e}"))
    })
}

fn bytes32_from_vec(label: &str, bytes: Vec<u8>) -> ProverResult<[u8; 32]> {
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        ProverError::GuestError(format!(
            "{label} must be 32 bytes, got {}",
            bytes.len()
        ))
    })
}

fn bytes32_to_words_le(bytes: &[u8; 32]) -> [u32; 8] {
    let mut words = [0u32; 8];
    for (idx, chunk) in bytes.chunks_exact(4).enumerate() {
        let chunk: [u8; 4] = chunk.try_into().expect("chunk size is 4");
        words[idx] = u32::from_le_bytes(chunk);
    }
    words
}
