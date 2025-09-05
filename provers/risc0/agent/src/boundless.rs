use std::time::Duration;
use tokio::time::timeout;
use std::str::FromStr;

use crate::methods::{
    boundless_aggregation::BOUNDLESS_AGGREGATION_ELF,
    boundless_batch::BOUNDLESS_BATCH_ELF,
};
use alloy_primitives_v1p2p0::{
    U256,
    utils::{parse_ether, parse_units},
};
use alloy_signer_local_v1p0p12::PrivateKeySigner;
use boundless_market::{
    Client, ProofRequest,
    contracts::RequestStatus,
    deployments::{BASE, Deployment, SEPOLIA},
    input::GuestEnv,
    request_builder::OfferParams,
};
use reqwest::Url;
use risc0_zkvm::{Digest, Receipt as ZkvmReceipt, default_executor};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;
use crate::storage::BoundlessStorage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProofRequestStatus {
    Submitted { market_request_id: U256 },
    Locked { market_request_id: U256, prover: Option<String> },
    Fulfilled { market_request_id: U256, proof: Vec<u8> },
    Failed { error: String },
}

/// Async proof request tracking
#[derive(Debug, Clone, Serialize)]
pub struct AsyncProofRequest {
    pub request_id: String,
    pub market_request_id: U256,
    pub status: ProofRequestStatus,
    pub proof_type: ProofType,
    pub input: Vec<u8>,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElfType {
    Batch,
    Aggregation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProofType {
    Batch,
    Aggregate,
    Update(ElfType),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeploymentType {
    Sepolia,
    Base,
}

impl FromStr for DeploymentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sepolia" | "SEPOLIA" => Ok(DeploymentType::Sepolia),
            "base" | "BASE" => Ok(DeploymentType::Base),
            _ => Err(format!(
                "Invalid deployment type: '{}'. Must be 'sepolia' or 'base'",
                s
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundlessAggregationGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<ZkvmReceipt>,
}

// use tokio::sync::OnceCell;

// Constants
const MAX_RETRY_ATTEMPTS: u32 = 5;
const MILLION_CYCLES: u64 = 1_000_000;
const STAKE_TOKEN_DECIMALS: u8 = 6;

/// Generic retry function with exponential backoff
async fn retry_with_backoff<F, Fut, T, E>(
    operation_name: &str,
    operation: F,
    max_retries: u32,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    let mut delay = Duration::from_secs(1); // Start with 1 second
    
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if attempt >= max_retries => {
                tracing::error!("{} failed after {} attempts: {}", operation_name, attempt, e);
                return Err(e);
            }
            Err(e) => {
                attempt += 1;
                tracing::warn!("{} failed (attempt {}/{}): {}, retrying in {:?}", 
                    operation_name, attempt, max_retries, e, delay);
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, Duration::from_secs(30)); // Cap at 30 seconds
            }
        }
    }
}

// now staking token is USDC, so we need to parse it as USDC whose decimals is 6
pub fn parse_staking_token(token: &str) -> AgentResult<U256> {
    let parsed = parse_units(token, STAKE_TOKEN_DECIMALS).map_err(|e| {
        AgentError::ClientBuildError(format!("Failed to parse stacking: {} ({})", token, e))
    })?;
    Ok(parsed.into())
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Risc0Response {
    pub seal: Vec<u8>,
    pub journal: Vec<u8>,
    pub receipt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoundlessOfferParams {
    pub ramp_up_sec: u32,
    pub lock_timeout_ms_per_mcycle: u32,
    pub timeout_ms_per_mcycle: u32,
    pub max_price_per_mcycle: String,
    pub min_price_per_mcycle: String,
    pub lock_stake: String,
}

impl Default for BoundlessOfferParams {
    fn default() -> Self {
        Self {
            ramp_up_sec: 200,
            lock_timeout_ms_per_mcycle: 1000,
            timeout_ms_per_mcycle: 3000,
            max_price_per_mcycle: "0.00001".to_string(),
            min_price_per_mcycle: "0.000003".to_string(),
            lock_stake: "0.0001".to_string(),
        }
    }
}

impl BoundlessOfferParams {
    fn aggregation() -> Self {
        Self {
            ramp_up_sec: 200,
            lock_timeout_ms_per_mcycle: 1000,
            timeout_ms_per_mcycle: 3000,
            max_price_per_mcycle: "0.00001".to_string(),
            min_price_per_mcycle: "0.000003".to_string(),
            lock_stake: "0.0001".to_string(),
        }
    }

    fn batch() -> Self {
        Self {
            ramp_up_sec: 1000,
            lock_timeout_ms_per_mcycle: 5000,
            timeout_ms_per_mcycle: 3600 * 3,
            max_price_per_mcycle: "0.00003".to_string(),
            min_price_per_mcycle: "0.000005".to_string(),
            lock_stake: "0.0001".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoundlessConfig {
    pub deployment: Option<DeploymentConfig>,
    pub offer_params: Option<OfferParamsConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentConfig {
    pub deployment_type: Option<DeploymentType>,
    pub overrides: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OfferParamsConfig {
    pub batch: Option<BoundlessOfferParams>,
    pub aggregation: Option<BoundlessOfferParams>,
}

impl Default for BoundlessConfig {
    fn default() -> Self {
        Self {
            deployment: Some(DeploymentConfig {
                deployment_type: Some(DeploymentType::Sepolia),
                overrides: None,
            }),
            offer_params: Some(OfferParamsConfig {
                batch: Some(BoundlessOfferParams::batch()),
                aggregation: Some(BoundlessOfferParams::aggregation()),
            }),
        }
    }
}

impl BoundlessConfig {
    /// Merge this config with another config, taking values from other where provided
    pub fn merge(&mut self, other: &BoundlessConfig) {
        // Merge deployment config if provided
        if let Some(other_deployment) = &other.deployment {
            if let Some(ref mut deployment) = self.deployment {
                // Merge deployment type
                if let Some(deployment_type) = &other_deployment.deployment_type {
                    deployment.deployment_type = Some(deployment_type.clone());
                }

                // Merge deployment overrides
                if let Some(other_overrides) = &other_deployment.overrides {
                    if let Some(ref mut overrides) = deployment.overrides {
                        // Merge JSON objects
                        if let (Some(obj1), Some(obj2)) =
                            (overrides.as_object_mut(), other_overrides.as_object())
                        {
                            for (key, value) in obj2 {
                                obj1.insert(key.clone(), value.clone());
                            }
                        }
                    } else {
                        deployment.overrides = Some(other_overrides.clone());
                    }
                }
            } else {
                self.deployment = Some(other_deployment.clone());
            }
        }

        // Merge offer params if provided
        if let Some(other_offer_params) = &other.offer_params {
            if let Some(ref mut offer_params) = self.offer_params {
                if let Some(batch) = &other_offer_params.batch {
                    offer_params.batch = Some(batch.clone());
                }
                if let Some(aggregation) = &other_offer_params.aggregation {
                    offer_params.aggregation = Some(aggregation.clone());
                }
            } else {
                self.offer_params = Some(other_offer_params.clone());
            }
        }
    }

    /// Get the effective deployment type, using default if not specified
    pub fn get_deployment_type(&self) -> DeploymentType {
        self.deployment
            .as_ref()
            .and_then(|d| d.deployment_type.as_ref())
            .cloned()
            .unwrap_or(DeploymentType::Sepolia)
    }

    /// Get the effective deployment configuration by merging with base deployment
    pub fn get_effective_deployment(&self) -> Deployment {
        let deployment_type = self.get_deployment_type();
        let mut deployment = match deployment_type {
            DeploymentType::Sepolia => SEPOLIA,
            DeploymentType::Base => BASE,
        };

        // Apply deployment overrides if provided
        if let Some(deployment_config) = &self.deployment {
            if let Some(overrides) = &deployment_config.overrides {
                if let Some(order_stream_url) =
                    overrides.get("order_stream_url").and_then(|v| v.as_str())
                {
                    deployment.order_stream_url =
                        Some(std::borrow::Cow::Owned(order_stream_url.to_string()));
                }
            }
        }

        deployment
    }

    /// Get the effective batch offer params, using default if not specified
    pub fn get_batch_offer_params(&self) -> BoundlessOfferParams {
        self.offer_params
            .as_ref()
            .and_then(|o| o.batch.as_ref())
            .cloned()
            .unwrap_or_else(BoundlessOfferParams::batch)
    }

    /// Get the effective aggregation offer params, using default if not specified
    pub fn get_aggregation_offer_params(&self) -> BoundlessOfferParams {
        self.offer_params
            .as_ref()
            .and_then(|o| o.aggregation.as_ref())
            .cloned()
            .unwrap_or_else(BoundlessOfferParams::aggregation)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProverConfig {
    pub offchain: bool,
    pub pull_interval: u64,
    pub rpc_url: String,
    pub boundless_config: BoundlessConfig,
    pub url_ttl: u64,
}

impl Default for ProverConfig {
    fn default() -> Self {
        ProverConfig {
            offchain: false,
            pull_interval: 10,
            rpc_url: "https://ethereum-sepolia-rpc.publicnode.com".to_string(),
            boundless_config: BoundlessConfig::default(),
            url_ttl: 1800,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BoundlessProver {
    batch_image_url: Arc<RwLock<Option<Url>>>,
    aggregation_image_url: Arc<RwLock<Option<Url>>>,
    config: ProverConfig,
    deployment: Deployment,
    boundless_config: BoundlessConfig,
    active_requests: Arc<RwLock<HashMap<String, AsyncProofRequest>>>,
    storage: BoundlessStorage,
}

// More specific error types
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Failed to build boundless client: {0}")]
    ClientBuildError(String),
    #[error("Failed to encode guest environment: {0}")]
    GuestEnvEncodeError(String),
    #[error("Failed to upload input: {0}")]
    UploadError(String),
    #[error("Failed to upload program: {0}")]
    ProgramUploadError(String),
    #[error("Failed to build request: {0}")]
    RequestBuildError(String),
    #[error("Failed to submit request: {0}")]
    RequestSubmitError(String),
    #[error("Failed to wait for request fulfillment after {attempts} attempts: {error}")]
    RequestFulfillmentError { attempts: u32, error: String },
    #[error("Failed to encode response: {0}")]
    ResponseEncodeError(String),
    #[error("Failed to execute guest environment: {0}")]
    GuestExecutionError(String),
    #[error("Did not receive requested unaggregated receipt")]
    InvalidReceiptError,
    #[error("Storage provider is required")]
    StorageProviderRequired,
}

pub type AgentResult<T> = Result<T, AgentError>;

impl BoundlessProver {
    /// Create a deployment based on the configuration
    fn create_deployment(config: &ProverConfig) -> AgentResult<Deployment> {
        Ok(config.boundless_config.get_effective_deployment())
    }

    /// Create a boundless client with the current configuration
    async fn create_boundless_client(&self) -> AgentResult<Client> {
        let deployment = Some(self.deployment.clone());
        let storage_provider = boundless_market::storage::storage_provider_from_env().ok();

        let url = Url::parse(&self.config.rpc_url).unwrap();
        let sender_priv_key = std::env::var("BOUNDLESS_SIGNER_KEY").unwrap_or_else(|_| {
            panic!("BOUNDLESS_SIGNER_KEY is not set");
        });
        let signer: PrivateKeySigner = sender_priv_key.parse().unwrap();

        let client = Client::builder()
            .with_rpc_url(url)
            .with_deployment(deployment)
            .with_storage_provider(storage_provider)
            .with_private_key(signer)
            .build()
            .await
            .map_err(|e| AgentError::ClientBuildError(e.to_string()))?;

        Ok(client)
    }

    /// Submit request to boundless market with retry logic
    async fn submit_request_async(
        &self,
        boundless_client: &Client,
        request: ProofRequest,
    ) -> AgentResult<U256> {
        // Send the request to the market with retry logic
        let request_id = if self.config.offchain {
            tracing::info!(
                "Submitting request offchain to {:?}",
                &self.deployment.order_stream_url
            );
            
            retry_with_backoff(
                "submit_request_offchain",
                || async {
                    boundless_client
                        .submit_request_offchain(&request)
                        .await
                        .map_err(|e| {
                            AgentError::RequestSubmitError(format!(
                                "Failed to submit request offchain: {e}"
                            ))
                        })
                },
                MAX_RETRY_ATTEMPTS,
            ).await?.0
        } else {
            retry_with_backoff(
                "submit_request_onchain",
                || async {
                    boundless_client
                        .submit_request_onchain(&request)
                        .await
                        .map_err(|e| {
                            AgentError::RequestSubmitError(format!("Failed to submit request onchain: {e}"))
                        })
                },
                MAX_RETRY_ATTEMPTS,
            ).await?.0
        };
        
        let request_id_str = format!("0x{:x}", request_id);
        tracing::info!("Request {} submitted successfully", request_id_str);
        
        Ok(request_id)
    }

    /// Check boundless market status and update request tracking
    async fn check_market_status(
        &self,
        market_request_id: U256,
    ) -> AgentResult<ProofRequestStatus> {
        let boundless_client = self.create_boundless_client().await?;
        let request_id_str = format!("0x{:x}", market_request_id);
        
        // First, check the current status using get_status with retry logic
        let status_result = retry_with_backoff(
            "get_market_status",
            || boundless_client.boundless_market.get_status(market_request_id, Some(u64::MAX)),
            3, // Fewer retries for status checks since we poll periodically
        ).await;
        
        match status_result {
            Ok(status) => {
                match status {
                    RequestStatus::Unknown => {
                        tracing::info!("Market status: MarketSubmitted({}) - open for bidding", request_id_str);
                        Ok(ProofRequestStatus::Submitted { 
                            market_request_id 
                        })
                    },
                    RequestStatus::Locked => {
                        tracing::info!("Market status: MarketLocked({}) - prover committed", request_id_str);
                        Ok(ProofRequestStatus::Locked { 
                            market_request_id, 
                            prover: None 
                        })
                    },
                    RequestStatus::Fulfilled => {
                        tracing::info!("Market status: MarketFulfilled({}) - proof completed", request_id_str);
                        
                        // Get the actual proof data with retry logic since we know it's fulfilled
                        let fulfillment_result = retry_with_backoff(
                            "get_request_fulfillment",
                            || boundless_client.boundless_market.get_request_fulfillment(market_request_id),
                            MAX_RETRY_ATTEMPTS,
                        ).await;
                        
                        match fulfillment_result {
                            Ok((journal, seal)) => {
                                let response = Risc0Response {
                                    seal: seal.to_vec(),
                                    journal: journal.to_vec(),
                                    receipt: None,
                                };
                                
                                let proof_bytes = bincode::serialize(&response).map_err(|e| {
                                    AgentError::ResponseEncodeError(format!("Failed to encode response: {e}"))
                                })?;
                                
                                Ok(ProofRequestStatus::Fulfilled { 
                                    market_request_id,
                                    proof: proof_bytes,
                                })
                            }
                            Err(e) => {
                                tracing::warn!("Failed to get fulfillment for {}: {}", request_id_str, e);
                                Ok(ProofRequestStatus::Failed { 
                                    error: format!("Failed to get proof data: {}", e),
                                })
                            }
                        }
                    },
                    RequestStatus::Expired => {
                        tracing::warn!("Market status: MarketExpired({}) - request expired", request_id_str);
                        Ok(ProofRequestStatus::Failed { 
                            error: "Request expired in boundless market".to_string(),
                        })
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get market status for {}: {}", request_id_str, e);
                Ok(ProofRequestStatus::Failed { 
                    error: format!("Failed to check market status: {}", e),
                })
            }
        }
    }


    /// Process input and create guest environment
    fn process_input(&self, input: Vec<u8>) -> AgentResult<(GuestEnv, Vec<u8>)> {
        let guest_env = GuestEnv::builder().write_frame(&input).build_env();
        let guest_env_bytes = guest_env.clone().encode().map_err(|e| {
            AgentError::ClientBuildError(format!("Failed to encode guest environment: {e}"))
        })?;
        Ok((guest_env, guest_env_bytes))
    }

    pub async fn new(config: ProverConfig) -> AgentResult<Self> {
        let deployment = BoundlessProver::create_deployment(&config)?;
        tracing::info!("boundless deployment: {:?}", deployment);

        // Create a temporary instance to use the create_boundless_client method
        // Initialize SQLite storage
        let db_path = std::env::var("SQLITE_DB_PATH")
            .unwrap_or_else(|_| "/data/boundless_requests.db".to_string());
        let storage = BoundlessStorage::new(db_path);
        storage.initialize().await?;

        // Clean up expired requests from previous runs
        match storage.delete_expired_requests().await {
            Ok(deleted_ids) => {
                if !deleted_ids.is_empty() {
                    tracing::info!("Cleaned up {} expired requests from previous runs", deleted_ids.len());
                }
            }
            Err(e) => tracing::warn!("Failed to clean up expired requests: {}", e),
        }

        let temp_prover = BoundlessProver {
            batch_image_url: Arc::new(RwLock::new(None)),
            aggregation_image_url: Arc::new(RwLock::new(None)),
            config: config.clone(),
            deployment: deployment.clone(),
            boundless_config: config.boundless_config.clone(),
            active_requests: Arc::new(RwLock::new(HashMap::new())),
            storage: storage.clone(),
        };

        let boundless_client = temp_prover.create_boundless_client().await?;

        // Upload the ELF to the storage provider so that it can be fetched by the market.
        if boundless_client.storage_provider.is_none() {
            return Err(AgentError::StorageProviderRequired);
        }

        // Always upload batch ELF (no caching)
        tracing::info!("Uploading batch ELF...");
        let batch_image_url = boundless_client
            .upload_program(BOUNDLESS_BATCH_ELF)
            .await
            .map_err(|e| AgentError::ProgramUploadError(format!("BOUNDLESS_BATCH_ELF: {e}")))?;

        // Always upload aggregation ELF (no caching)
        tracing::info!("Uploading aggregation ELF...");
        let aggregation_image_url = boundless_client
            .upload_program(BOUNDLESS_AGGREGATION_ELF)
            .await
            .map_err(|e| AgentError::ProgramUploadError(format!("BOUNDLESS_AGGREGATION_ELF: {e}")))?;

        let final_prover = BoundlessProver {
            batch_image_url: Arc::new(RwLock::new(Some(batch_image_url))),
            aggregation_image_url: Arc::new(RwLock::new(Some(aggregation_image_url))),
            config,
            deployment,
            boundless_config: temp_prover.boundless_config.clone(),
            active_requests: Arc::new(RwLock::new(HashMap::new())),
            storage: storage.clone(),
        };


        Ok(final_prover)
    }

    pub async fn get_batch_image_url(&self) -> Option<Url> {
        self.batch_image_url.read().await.clone()
    }

    pub async fn get_aggregation_image_url(&self) -> Option<Url> {
        self.aggregation_image_url.read().await.clone()
    }

    pub fn prover_config(&self) -> ProverConfig {
        self.config.clone()
    }



    /// Helper method to prepare and store async request
    async fn prepare_async_request(
        &self,
        request_id: String,
        proof_type: ProofType,
        input: Vec<u8>,
        config: &serde_json::Value,
    ) -> AgentResult<String> {
        tracing::info!("Preparing {} proof request: {}", 
            match proof_type { 
                ProofType::Batch => "batch",
                ProofType::Aggregate => "aggregation",
                ProofType::Update(_) => "update"
            }, request_id);

        let async_request = AsyncProofRequest {
            request_id: request_id.clone(),
            market_request_id: U256::ZERO, // Will be set when submitted
            status: ProofRequestStatus::Submitted { 
                market_request_id: U256::ZERO 
            },
            proof_type,
            input,
            config: config.clone(),
        };

        // Store the request for tracking (both memory and SQLite)
        {
            let mut requests_guard = self.active_requests.write().await;
            requests_guard.insert(request_id.clone(), async_request.clone());
        }
        
        // Persist to SQLite storage
        if let Err(e) = self.storage.store_request(&async_request).await {
            tracing::warn!("Failed to store {} request in SQLite: {}", 
                match async_request.proof_type { 
                    ProofType::Batch => "batch",
                    ProofType::Aggregate => "aggregation",
                    ProofType::Update(_) => "update"
                }, e);
        }

        Ok(request_id)
    }

    /// Helper method to update failed status in both memory and storage
    async fn update_failed_status(&self, request_id: &str, error: String) {
        let failed_status = ProofRequestStatus::Failed { error };
        let _ = self.update_request_status(request_id, failed_status, &self.active_requests).await;
    }

    /// Helper method to update request status in both memory and storage
    async fn update_request_status(
        &self,
        request_id: &str,
        status: ProofRequestStatus,
        active_requests: &Arc<RwLock<HashMap<String, AsyncProofRequest>>>,
    ) -> AgentResult<()> {
        // Update status in memory
        {
            let mut requests_guard = active_requests.write().await;
            if let Some(async_req) = requests_guard.get_mut(request_id) {
                async_req.status = status.clone();
            }
        }
        
        // Update in SQLite storage
        if let Err(e) = self.storage.update_status(request_id, &status).await {
            tracing::warn!("Failed to update status in storage: {}", e);
            return Err(AgentError::ClientBuildError(format!("Storage update failed: {}", e)));
        }

        Ok(())
    }

    /// Helper method to perform a single market status poll
    async fn poll_market_status(
        &self,
        request_id: &str,
        market_request_id: U256,
        active_requests: &Arc<RwLock<HashMap<String, AsyncProofRequest>>>,
    ) -> bool {
        let market_id_str = format!("0x{:x}", market_request_id);
        
        // Use retry logic for status polling to handle transient failures
        let status_result = retry_with_backoff(
            "check_market_status_polling",
            || self.check_market_status(market_request_id),
            3, // Fewer retries since we poll periodically
        ).await;
        
        match status_result {
            Ok(new_status) => {
                // Update the status using the helper
                if let Err(e) = self.update_request_status(request_id, new_status.clone(), active_requests).await {
                    tracing::warn!("Failed to update status for {}: {}", request_id, e);
                }
                
                // Check if we should stop polling (fulfilled or failed)
                match new_status {
                    ProofRequestStatus::Fulfilled { .. } => {
                        tracing::info!("Proof {} completed via market", market_id_str);
                        false // Stop polling
                    }
                    ProofRequestStatus::Failed { .. } => {
                        tracing::error!("Proof {} failed via market", market_id_str);
                        false // Stop polling
                    }
                    _ => {
                        true // Continue polling
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check market status for {}: {}", market_id_str, e);
                true // Continue polling despite error
            }
        }
    }

    /// Helper method to handle polling timeout
    async fn handle_polling_timeout(
        &self,
        request_id: &str,
        active_requests: Arc<RwLock<HashMap<String, AsyncProofRequest>>>,
    ) {
        tracing::warn!("Request {} timed out after 1 hour, marking as failed", request_id);
        
        let timeout_status = ProofRequestStatus::Failed {
            error: "Request timed out after 1 hour".to_string(),
        };
        
        // Update status using helper
        let _ = self.update_request_status(request_id, timeout_status, &active_requests).await;
        
        // Remove from memory
        let mut requests_guard = active_requests.write().await;
        requests_guard.remove(request_id);
        
        tracing::info!("Removed timed out request {} from memory", request_id);
    }

    /// Helper method to start status polling for market requests
    async fn start_status_polling(
        &self,
        request_id: &str,
        market_request_id: U256,
        active_requests: Arc<RwLock<HashMap<String, AsyncProofRequest>>>,
    ) {
        let prover_clone = self.clone();
        let request_id = request_id.to_string();
        
        tokio::spawn(async move {
            let poll_interval = Duration::from_secs(10);
            
            // Create the polling future
            let pollings = async {
                while prover_clone
                    .poll_market_status(&request_id, market_request_id, &active_requests)
                    .await 
                {
                    tokio::time::sleep(poll_interval).await;
                }
            };
            
            // Use timeout wrapper as suggested
            match timeout(Duration::from_secs(3600), pollings).await {
                Ok(_) => {
                    tracing::info!("Polling finished before timeout for request {}", request_id);
                }
                Err(_) => {
                    prover_clone.handle_polling_timeout(&request_id, active_requests).await;
                }
            }
        });
    }

    /// Helper method to process input, build request, and submit to market
    async fn process_and_submit_request(
        &self,
        request_id: &str,
        input: Vec<u8>,
        elf: &[u8],
        image_url: Url,
        offer_params: BoundlessOfferParams,
        active_requests: Arc<RwLock<HashMap<String, AsyncProofRequest>>>,
    ) -> AgentResult<()> {
        
        let boundless_client = retry_with_backoff(
            "create_boundless_client",
            || self.create_boundless_client(),
            3, // Fewer retries for client creation
        ).await.map_err(|e| AgentError::ClientBuildError(format!("Failed to create boundless client: {}", e)))?;

        // Process input and create guest environment
        let (guest_env, guest_env_bytes) = self.process_input(input)
            .map_err(|e| AgentError::GuestEnvEncodeError(format!("Failed to process input: {}", e)))?;

        // Evaluate cost
        let (mcycles_count, _) = self.evaluate_cost(&guest_env, elf).await
            .map_err(|e| AgentError::GuestExecutionError(format!("Failed to evaluate cost: {}", e)))?;

        // Upload input if large enough
        const INPUT_SIZE_THRESHOLD: usize = 1024 * 1024; // 1MB
        let input_url = if guest_env_bytes.len() > INPUT_SIZE_THRESHOLD {
            tracing::info!("Input size {} bytes exceeds threshold, uploading to storage provider", guest_env_bytes.len());
            Some(retry_with_backoff(
                "upload_input",
                || boundless_client.upload_input(&guest_env_bytes),
                MAX_RETRY_ATTEMPTS,
            ).await.map_err(|e| AgentError::UploadError(format!("Failed to upload input: {}", e)))?)
        } else {
            tracing::info!("Input size {} bytes is small, using inline", guest_env_bytes.len());
            None
        };

        // Build the request
        let request = self.build_boundless_request(
            &boundless_client,
            image_url,
            input_url,
            guest_env,
            &offer_params,
            mcycles_count as u32,
        ).await.map_err(|e| AgentError::RequestBuildError(format!("Failed to build request: {}", e)))?;

        // Submit to market
        let market_request_id = self.submit_request_async(&boundless_client, request).await
            .map_err(|e| AgentError::RequestSubmitError(format!("Failed to submit to market: {}", e)))?;

        // Update the stored request with new market_request_id
        {
            let mut requests_guard = active_requests.write().await;
            if let Some(async_req) = requests_guard.get_mut(request_id) {
                async_req.market_request_id = market_request_id;
                async_req.status = ProofRequestStatus::Submitted { 
                    market_request_id 
                };
            }
        }

        // Update in SQLite storage with correct market_request_id
        let submitted_status = ProofRequestStatus::Submitted { 
            market_request_id 
        };
        if let Err(e) = self.storage.update_status(request_id, &submitted_status).await {
            tracing::warn!("Failed to update market request ID in storage: {}", e);
        }

        // Start polling market status in background
        self.start_status_polling(request_id, market_request_id, active_requests).await;

        Ok(())
    }

    /// Submit a batch proof request asynchronously
    pub async fn batch_run(
        &self,
        request_id: String,
        input: Vec<u8>,
        config: &serde_json::Value,
    ) -> AgentResult<String> {
        // Check for existing request with same input
        if let Some(existing_request) = self.storage.get_request_by_input_hash(&input, &ProofType::Batch).await? {
            match &existing_request.status {
                ProofRequestStatus::Fulfilled { .. } => {
                    tracing::info!("Returning existing completed batch proof for request: {}", existing_request.request_id);
                    return Ok(existing_request.request_id);
                },
                ProofRequestStatus::Submitted { .. } => {
                    tracing::info!("Returning existing submitted batch proof (waiting for prover) for request: {}", existing_request.request_id);
                    // Add to memory cache if not already there
                    {
                        let mut requests_guard = self.active_requests.write().await;
                        if !requests_guard.contains_key(&existing_request.request_id) {
                            requests_guard.insert(existing_request.request_id.clone(), existing_request.clone());
                        }
                    }
                    return Ok(existing_request.request_id);
                },
                ProofRequestStatus::Locked { .. } => {
                    tracing::info!("Returning existing locked batch proof (being processed by prover) for request: {}", existing_request.request_id);
                    // Add to memory cache if not already there
                    {
                        let mut requests_guard = self.active_requests.write().await;
                        if !requests_guard.contains_key(&existing_request.request_id) {
                            requests_guard.insert(existing_request.request_id.clone(), existing_request.clone());
                        }
                    }
                    return Ok(existing_request.request_id);
                },
                ProofRequestStatus::Failed { error } => {
                    tracing::info!("Found failed request for same input ({}), creating new batch request", error);
                    // Continue to create new request (allows retry)
                }
            }
        }

        // Prepare and store the async request
        let request_id = self.prepare_async_request(
            request_id,
            ProofType::Batch,
            input.clone(),
            config,
        ).await?;

        // Submit to boundless market in background
        let prover_clone = self.clone();
        let active_requests = self.active_requests.clone();
        let request_id_clone = request_id.clone();

        tokio::spawn(async move {
            let offer_params = prover_clone.boundless_config.get_batch_offer_params();
            let image_url = prover_clone.batch_image_url.read().await.clone().unwrap();

            if let Err(e) = prover_clone.process_and_submit_request(
                &request_id_clone,
                input,
                BOUNDLESS_BATCH_ELF,
                image_url,
                offer_params,
                active_requests,
            ).await {
                prover_clone.update_failed_status(&request_id_clone, e.to_string()).await;
            }
        });

        Ok(request_id)
    }

    /// Submit an aggregation proof request asynchronously
    pub async fn aggregate(
        &self,
        request_id: String,
        input: Vec<u8>,
        config: &serde_json::Value,
    ) -> AgentResult<String> {
        // Check for existing request with same input
        if let Some(existing_request) = self.storage.get_request_by_input_hash(&input, &ProofType::Aggregate).await? {
            match &existing_request.status {
                ProofRequestStatus::Fulfilled { .. } => {
                    tracing::info!("Returning existing completed aggregation proof for request: {}", existing_request.request_id);
                    return Ok(existing_request.request_id);
                },
                ProofRequestStatus::Submitted { .. } => {
                    tracing::info!("Returning existing submitted aggregation proof (waiting for prover) for request: {}", existing_request.request_id);
                    // Add to memory cache if not already there
                    {
                        let mut requests_guard = self.active_requests.write().await;
                        if !requests_guard.contains_key(&existing_request.request_id) {
                            requests_guard.insert(existing_request.request_id.clone(), existing_request.clone());
                        }
                    }
                    return Ok(existing_request.request_id);
                },
                ProofRequestStatus::Locked { .. } => {
                    tracing::info!("Returning existing locked aggregation proof (being processed by prover) for request: {}", existing_request.request_id);
                    // Add to memory cache if not already there
                    {
                        let mut requests_guard = self.active_requests.write().await;
                        if !requests_guard.contains_key(&existing_request.request_id) {
                            requests_guard.insert(existing_request.request_id.clone(), existing_request.clone());
                        }
                    }
                    return Ok(existing_request.request_id);
                },
                ProofRequestStatus::Failed { error } => {
                    tracing::info!("Found failed request for same input ({}), creating new aggregation request", error);
                    // Continue to create new request (allows retry)
                }
            }
        }

        // Prepare and store the async request
        let request_id = self.prepare_async_request(
            request_id,
            ProofType::Aggregate,
            input.clone(),
            config,
        ).await?;

        // Submit to boundless market in background
        let prover_clone = self.clone();
        let active_requests = self.active_requests.clone();
        let request_id_clone = request_id.clone();

        tokio::spawn(async move {
            let offer_params = prover_clone.boundless_config.get_aggregation_offer_params();
            let image_url = prover_clone.aggregation_image_url.read().await.clone().unwrap();

            if let Err(e) = prover_clone.process_and_submit_request(
                &request_id_clone,
                input,
                BOUNDLESS_AGGREGATION_ELF,
                image_url,
                offer_params,
                active_requests,
            ).await {
                prover_clone.update_failed_status(&request_id_clone, e.to_string()).await;
            }
        });

        Ok(request_id)
    }

    /// update elf
    pub async fn update(
        &self,
        _request_id: String,
        _elf: Vec<u8>,
        _elf_type: ElfType,
    ) -> AgentResult<String> {
        todo!()
    }

    /// Get the current status of an async request
    pub async fn get_request_status(&self, request_id: &str) -> Option<AsyncProofRequest> {
        // Try to get from SQLite storage first (most up-to-date)
        match self.storage.get_request(request_id).await {
            Ok(Some(request)) => {
                // Also update memory cache
                let mut requests_guard = self.active_requests.write().await;
                requests_guard.insert(request_id.to_string(), request.clone());
                Some(request)
            }
            Ok(None) => {
                // Not found in storage, try memory
                let requests_guard = self.active_requests.read().await;
                requests_guard.get(request_id).cloned()
            }
            Err(e) => {
                tracing::warn!("Failed to get request from storage, falling back to memory: {}", e);
                let requests_guard = self.active_requests.read().await;
                requests_guard.get(request_id).cloned()
            }
        }
    }

    /// List all active requests
    pub async fn list_active_requests(&self) -> Vec<AsyncProofRequest> {
        // Get from SQLite storage for most up-to-date data
        match self.storage.list_active_requests().await {
            Ok(requests) => requests,
            Err(e) => {
                tracing::warn!("Failed to get requests from storage, falling back to memory: {}", e);
                let requests_guard = self.active_requests.read().await;
                requests_guard.values().cloned().collect()
            }
        }
    }

    /// Get database statistics for monitoring
    pub async fn get_database_stats(&self) -> AgentResult<crate::storage::DatabaseStats> {
        self.storage.get_stats().await
    }

    /// Delete all requests from the database
    /// Returns the number of deleted requests
    pub async fn delete_all_requests(&self) -> AgentResult<usize> {
        let deleted_count = self.storage.delete_all_requests().await?;
        
        // Clear in-memory active requests as well
        self.active_requests.write().await.clear();
        
        tracing::info!("Deleted {} requests from database and cleared memory cache", deleted_count);
        Ok(deleted_count)
    }

    async fn evaluate_cost(&self, guest_env: &GuestEnv, elf: &[u8]) -> AgentResult<(u64, Vec<u8>)> {
        let (mcycles_count, _journal) = {
            // Dry run the ELF with the input to get the journal and cycle count.
            // This can be useful to estimate the cost of the proving request.
            // It can also be useful to ensure the guest can be executed correctly and we do not send into
            // the market unprovable proving requests. If you have a different mechanism to get the expected
            // journal and set a price, you can skip this step.
            let session_info = default_executor()
                .execute(guest_env.clone().try_into().unwrap(), elf)
                .map_err(|e| {
                    AgentError::GuestExecutionError(format!(
                        "Failed to execute guest environment: {e}"
                    ))
                })?;
            let mcycles_count = session_info
                .segments
                .iter()
                .map(|segment| 1 << segment.po2)
                .sum::<u64>()
                .div_ceil(MILLION_CYCLES);
            let journal = session_info.journal.bytes;
            (mcycles_count, journal)
        };
        tracing::info!("mcycles_count: {}", mcycles_count);
        Ok((mcycles_count, _journal))
    }

    async fn build_boundless_request(
        &self,
        boundless_client: &Client,
        program_url: Url,
        _input_url: Option<Url>,
        guest_env: GuestEnv,
        offer_spec: &BoundlessOfferParams,
        mcycles_count: u32,
    ) -> AgentResult<ProofRequest> {
        tracing::info!("offer_spec: {:?}", offer_spec);
        let max_price = parse_ether(&offer_spec.max_price_per_mcycle).map_err(|e| {
            AgentError::ClientBuildError(format!(
                "Failed to parse max_price_per_mcycle: {} ({})",
                offer_spec.max_price_per_mcycle, e
            ))
        })? * U256::from(mcycles_count);

        // let min_price = parse_ether(&offer_spec.min_price_per_mcycle).map_err(|e| {
        //     AgentError::ClientBuildError(format!(
        //         "Failed to parse min_price_per_mcycle: {} ({})",
        //         offer_spec.min_price_per_mcycle, e
        //     ))
        // })? * U256::from(mcycles_count);

        let lock_stake = parse_staking_token(&offer_spec.lock_stake)?;
        let lock_timeout = (offer_spec.lock_timeout_ms_per_mcycle * mcycles_count / 1000u32) as u32;
        let timeout = (offer_spec.timeout_ms_per_mcycle * mcycles_count / 1000u32) as u32;
        let ramp_up_period = std::cmp::min(offer_spec.ramp_up_sec, lock_timeout);

        let request_params = boundless_client
            .new_request()
            .with_program_url(program_url)
            .unwrap()
            .with_groth16_proof()
            .with_env(guest_env)
            .with_cycles(mcycles_count as u64 * MILLION_CYCLES)
            // .with_input_url(input_url)
            // .with_env(GuestEnv::builder().write_frame(&guest_env_bytes))
            // .unwrap()
            .with_offer(
                OfferParams::builder()
                    .ramp_up_period(ramp_up_period)
                    .lock_timeout(lock_timeout)
                    .timeout(timeout)
                    .max_price(max_price)
                    // .min_price(min_price)
                    .lock_stake(lock_stake),
            );

        // Build the request, including preflight, and assigned the remaining fields.
        let mut request = boundless_client
            .build_request(request_params)
            .await
            .map_err(|e| AgentError::ClientBuildError(format!("Failed to build request: {e:?}")))?;
        tracing::info!("Request: {:?}", request);

        // give 60s to the market to accept the request
        request.offer = request.offer.clone().with_bidding_start(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 60,
        );
        Ok(request)
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
        BoundlessProver::new(ProverConfig::default())
            .await
            .unwrap();
    }

    #[test]
    fn test_deployment_selection() {
        // Test Sepolia deployment
        let mut config = ProverConfig::default();
        config.boundless_config.deployment = Some(DeploymentConfig {
            deployment_type: Some(DeploymentType::Sepolia),
            overrides: None,
        });
        let deployment = BoundlessProver::create_deployment(&config).unwrap();
        assert!(deployment.order_stream_url.is_none() || deployment.order_stream_url.is_some());

        // Test Base deployment
        config.boundless_config.deployment = Some(DeploymentConfig {
            deployment_type: Some(DeploymentType::Base),
            overrides: None,
        });
        let deployment = BoundlessProver::create_deployment(&config).unwrap();
        assert!(deployment.order_stream_url.is_none() || deployment.order_stream_url.is_some());
    }

    #[test]
    fn test_deployment_type_from_str() {
        // Test valid deployment types
        assert_eq!(
            DeploymentType::from_str("sepolia").unwrap(),
            DeploymentType::Sepolia
        );
        assert_eq!(
            DeploymentType::from_str("base").unwrap(),
            DeploymentType::Base
        );

        // Test case insensitive
        assert_eq!(
            DeploymentType::from_str("SEPOLIA").unwrap(),
            DeploymentType::Sepolia
        );
        assert_eq!(
            DeploymentType::from_str("BASE").unwrap(),
            DeploymentType::Base
        );

        // Test invalid deployment types
        assert!(DeploymentType::from_str("invalid").is_err());
        assert!(DeploymentType::from_str("").is_err());
    }

    #[tokio::test]
    async fn test_run_prover() {
        // init log
        env_logger::init();

        // loading from tests/fixtures/input-1306738.bin
        let input_bytes = std::fs::read("tests/fixtures/input-1306738.bin").unwrap();
        let output_bytes = std::fs::read("tests/fixtures/output-1306738.bin").unwrap();

        let config = serde_json::Value::default();
        let prover = BoundlessProver::new(ProverConfig::default())
            .await
            .unwrap();
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

    #[ignore = "not needed in CI"]
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
        let prover = BoundlessProver::new(ProverConfig::default())
            .await
            .unwrap();
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

    #[ignore]
    #[test]
    fn test_deserialize_boundless_config() {
        // Create test config
        let config = BoundlessConfig {
            deployment: Some(DeploymentConfig {
                deployment_type: Some(DeploymentType::Sepolia),
                overrides: None,
            }),
            offer_params: Some(OfferParamsConfig {
                batch: Some(BoundlessOfferParams::batch()),
                aggregation: Some(BoundlessOfferParams::aggregation()),
            }),
        };

        // Test serialization and deserialization
        let config_json = serde_json::to_string(&config).unwrap();
        let deserialized_config: BoundlessConfig = serde_json::from_str(&config_json).unwrap();

        // Verify the config was deserialized correctly
        assert_eq!(
            deserialized_config.get_deployment_type(),
            DeploymentType::Sepolia
        );

        println!("Deserialized config: {:#?}", deserialized_config);
    }

    #[test]
    fn test_prover_config_with_boundless_config() {
        let boundless_config = BoundlessConfig {
            deployment: Some(DeploymentConfig {
                deployment_type: Some(DeploymentType::Base),
                overrides: None,
            }),
            offer_params: Some(OfferParamsConfig {
                batch: Some(BoundlessOfferParams::batch()),
                aggregation: Some(BoundlessOfferParams::aggregation()),
            }),
        };

        let prover_config = ProverConfig {
            offchain: true,
            pull_interval: 15,
            rpc_url: "https://custom-rpc.com".to_string(),
            boundless_config,
            url_ttl: 1800,
        };

        // Test that the deployment is created correctly from boundless_config
        let deployment = BoundlessProver::create_deployment(&prover_config).unwrap();
        // Base deployment should have its default order_stream_url
        assert!(deployment.order_stream_url.is_some());
    }

    #[test]
    fn test_partial_config_override() {
        // Create a config that only overrides deployment type
        let partial_config = BoundlessConfig {
            deployment: Some(DeploymentConfig {
                deployment_type: Some(DeploymentType::Base),
                overrides: None,
            }),
            offer_params: None,
        };

        // Start with default config
        let mut default_config = BoundlessConfig::default();

        // Merge the partial config
        default_config.merge(&partial_config);

        // Verify that deployment type was overridden
        assert_eq!(default_config.get_deployment_type(), DeploymentType::Base);

        // Verify that offer params still use defaults
        let batch_params = default_config.get_batch_offer_params();
        let aggregation_params = default_config.get_aggregation_offer_params();

        // These should match the default values
        assert_eq!(batch_params.ramp_up_sec, 1000);
        assert_eq!(aggregation_params.ramp_up_sec, 200);
    }

    #[test]
    fn test_deployment_overrides() {
        // Test deployment overrides functionality
        let overrides = serde_json::json!({
            "order_stream_url": "https://custom-order-stream.com",
        });

        let config = BoundlessConfig {
            deployment: Some(DeploymentConfig {
                deployment_type: Some(DeploymentType::Sepolia),
                overrides: Some(overrides),
            }),
            offer_params: None,
        };

        let deployment = config.get_effective_deployment();

        // Verify that the overrides were applied
        assert_eq!(
            deployment.order_stream_url,
            Some(std::borrow::Cow::Owned(
                "https://custom-order-stream.com".to_string()
            ))
        );
    }

    #[test]
    fn test_offer_params_max_price() {
        let offer_params = BoundlessOfferParams::batch();
        let max_price_per_mcycle = parse_ether(&offer_params.max_price_per_mcycle)
            .expect("Failed to parse max_price_per_mcycle");
        let max_price = max_price_per_mcycle * U256::from(1000u64);
        // 0.00003 * 1000 = 0.03 ETH
        assert_eq!(max_price, U256::from(30000000000000000u128));

        let min_price_per_mcycle = parse_ether(&offer_params.min_price_per_mcycle)
            .expect("Failed to parse min_price_per_mcycle");
        let min_price = min_price_per_mcycle * U256::from(1000u64);
        // 0.000005 * 1000 = 0.005 ETH
        assert_eq!(min_price, U256::from(5000000000000000u128));

        let lock_stake_per_mcycle = parse_staking_token(&offer_params.lock_stake)
            .expect("Failed to parse lock_stake_per_mcycle");
        let lock_stake = lock_stake_per_mcycle * U256::from(1000u64);
        // 0.0001 * 1000 = 0.1 USDC
        assert_eq!(lock_stake, U256::from(100000u64));
    }
}
