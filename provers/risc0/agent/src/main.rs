pub mod boundless;
pub mod storage;
pub use boundless::{
    AgentError, AgentResult, AsyncProofRequest, DeploymentType, ElfType, 
    ProofRequestStatus, ProverConfig, BoundlessProver, ProofType as BoundlessProofType,
};
pub use storage::{BoundlessStorage, DatabaseStats};

pub mod methods;

use axum::{
    Json,
    extract::{DefaultBodyLimit, State},
};
use axum::{Router, http::StatusCode, routing::{post, get, delete}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use alloy_primitives_v1p2p0::U256;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ProofType {
    Batch,
    Aggregate,
    Update(ElfType),
}


#[derive(Debug, Clone)]
struct AppState {
    prover: Arc<Mutex<Option<BoundlessProver>>>,
    prover_init_time: Arc<Mutex<Option<std::time::Instant>>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            prover: Arc::new(Mutex::new(None)),
            prover_init_time: Arc::new(Mutex::new(None)),
        }
    }

    async fn init_prover(&self, config: ProverConfig) -> AgentResult<BoundlessProver> {
        let prover = BoundlessProver::new(config).await.map_err(|e| {
            AgentError::ClientBuildError(format!("Failed to initialize prover: {}", e))
        })?;
        self.prover.lock().await.replace(prover.clone());
        self.prover_init_time
            .lock()
            .await
            .replace(std::time::Instant::now());
        Ok(prover)
    }

    /// Get the prover, re-initializing if TTL (3600s) has expired.
    async fn get_or_refresh_prover(&self) -> AgentResult<BoundlessProver> {
        let mut prover_guard = self.prover.lock().await;
        let config_guard = prover_guard.as_ref().unwrap().prover_config();
        let mut time_guard = self.prover_init_time.lock().await;
        let now = std::time::Instant::now();
        let ttl = std::time::Duration::from_secs(config_guard.url_ttl);

        let should_refresh = match *time_guard {
            Some(init_time) => now.duration_since(init_time) > ttl,
            None => true,
        };

        if should_refresh || prover_guard.is_none() {
            tracing::info!("Prover TTL exceeded or not initialized, re-initializing prover...");
            let prover = BoundlessProver::new(config_guard).await.map_err(|e| {
                AgentError::ClientBuildError(format!("Failed to initialize prover: {}", e))
            })?;
            *prover_guard = Some(prover.clone());
            *time_guard = Some(now);
            Ok(prover)
        } else {
            Ok(prover_guard.as_ref().unwrap().clone())
        }
    }
}


async fn health_check() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "service": "boundless-agent"
        })),
    )
}

#[derive(Debug, Deserialize)]
struct AsyncProofRequestData {
    request_id: String,
    input: Vec<u8>,
    proof_type: ProofType,
    elf: Option<Vec<u8>>,
    config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AsyncProofResponse {
    request_id: String,
    market_request_id: U256,
    status: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct DetailedStatusResponse {
    request_id: String,
    market_request_id: U256,
    status: String,
    status_message: String,
    proof_data: Option<Vec<u8>>, // Raw proof bytes when completed
    error: Option<String>,
}


/// Convert internal ProofRequestStatus to user-friendly API response
fn map_status_to_api_response(request: &AsyncProofRequest) -> DetailedStatusResponse {
    let (status, status_message, proof_data, error) = match &request.status {
        ProofRequestStatus::Submitted { .. } => (
            "submitted".to_string(),
            "Your proof request has been submitted to the boundless market and is waiting for an available prover to pick it up.".to_string(),
            None,
            None,
        ),
        ProofRequestStatus::Locked { prover, .. } => (
            "in_progress".to_string(),
            format!("A prover {} has accepted your request and is generating the proof.", 
                prover.as_ref().map(|p| format!("({})", p)).unwrap_or_else(|| "".to_string())),
            None,
            None,
        ),
        ProofRequestStatus::Fulfilled { proof, .. } => (
            "completed".to_string(),
            "Your proof has been successfully generated and is ready for download.".to_string(),
            Some(proof.clone()),
            None,
        ),
        ProofRequestStatus::Failed { error } => (
            "failed".to_string(),
            format!("Proof generation failed: {}", error),
            None,
            Some(error.clone()),
        ),
    };

    DetailedStatusResponse {
        request_id: request.request_id.clone(),
        market_request_id: request.market_request_id,
        status,
        status_message,
        proof_data,
        error,
    }
}

async fn proof_handler(
    State(state): State<AppState>,
    Json(request): Json<AsyncProofRequestData>,
) -> (StatusCode, Json<AsyncProofResponse>) {
    tracing::info!(
        "Received async proof submission: {} (size: {} bytes)",
        request.request_id,
        request.input.len()
    );

    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(e) => {
            tracing::error!("Failed to get prover: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AsyncProofResponse {
                    request_id: request.request_id,
                    market_request_id: U256::ZERO,
                    status: "error".to_string(),
                    message: format!("Failed to initialize prover: {}", e),
                }),
            );
        }
    };

    let config = request.config.unwrap_or_else(|| serde_json::Value::default());
    
    // Convert ProofType to BoundlessProofType and call appropriate async method
    let result = match request.proof_type {
        ProofType::Batch => {
            prover.batch_run(request.request_id.clone(), request.input, &config).await
        }
        ProofType::Aggregate => {
            prover.aggregate(request.request_id.clone(), request.input, &config).await
        }
        ProofType::Update(elf_type) => {
            match request.elf {
                Some(elf_data) => {
                    prover.update(request.request_id.clone(), elf_data, elf_type).await
                }
                None => {
                    Err(AgentError::RequestBuildError("ELF data required for Update proof type".to_string()))
                }
            }
        }
    };
    
    match result {
        Ok(async_request_id) => {
            tracing::info!("Async proof already submitted with ID: {}", async_request_id);
            (
                StatusCode::ACCEPTED,
                Json(AsyncProofResponse {
                    request_id: async_request_id,
                    market_request_id: U256::ZERO,
                    status: "submitted".to_string(),
                    message: "Proof request submitted for async processing".to_string(),
                }),
            )
        }
        Err(e) => {
            tracing::error!("Failed to submit async proof: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AsyncProofResponse {
                    request_id: request.request_id,
                    market_request_id: U256::ZERO,
                    status: "error".to_string(),
                    message: format!("Failed to submit proof: {}", e),
                }),
            )
        }
    }
}

async fn get_async_proof_status(
    State(state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to get prover: {}", e)
                })),
            );
        }
    };

    match prover.get_request_status(&request_id).await {
        Some(request) => {
            let detailed_response = map_status_to_api_response(&request);
            (StatusCode::OK, Json(serde_json::to_value(detailed_response).unwrap()))
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Request not found",
                "message": "No async proof request found with the specified market_request_id"
            })),
        ),
    }
}

async fn list_async_requests(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to get prover: {}", e)
                })),
            );
        }
    };

    let requests = prover.list_active_requests().await;
    let detailed_requests: Vec<DetailedStatusResponse> = requests
        .iter()
        .map(|req| map_status_to_api_response(req))
        .collect();
    
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "active_requests": requests.len(),
            "requests": detailed_requests
        })),
    )
}


/// Get database statistics for monitoring
async fn get_database_stats(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to get prover: {}", e)
                })),
            );
        }
    };

    match prover.get_database_stats().await {
        Ok(stats) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "database_stats": stats
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to get database stats: {}", e)
            })),
        ),
    }
}

/// Delete all requests from the database
async fn delete_all_requests(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to get prover: {}", e)
                })),
            );
        }
    };

    match prover.delete_all_requests().await {
        Ok(deleted_count) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "message": format!("Successfully deleted {} requests", deleted_count),
                "deleted_count": deleted_count
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to delete requests: {}", e)
            })),
        ),
    }
}

use clap::Parser;

/// Command line arguments for the RISC0 Boundless Agent
#[derive(Parser, Debug)]
#[command(name = "risc0-boundless-agent")]
#[command(about = "RISC0 Boundless Agent Web Service", long_about = None)]
struct CmdArgs {
    /// Address to bind the server to (e.g., 0.0.0.0)
    #[arg(long, default_value = "0.0.0.0")]
    address: String,

    /// Port to listen on
    #[arg(long, default_value_t = 9999)]
    port: u16,

    /// Enable offchain mode for the prover
    #[arg(long, default_value_t = false)]
    offchain: bool,

    /// RPC URL
    #[arg(long, default_value = "https://ethereum-sepolia-rpc.publicnode.com")]
    rpc_url: String,

    /// Pull interval
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(5..))]
    pull_interval: u64,

    /// URL TTL
    #[arg(long, default_value_t = 1800)]
    url_ttl: u64,

    /// singer key hex string
    #[arg(long)]
    signer_key: Option<String>,

    /// Path to boundless config file (JSON format)
    #[arg(long)]
    config_file: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    tracing::info!("Starting RISC0 Boundless Agent Web Service...");

    let args = CmdArgs::parse();
    tracing::info!("Input config: {:?}", args);

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Create app state
    let state = AppState::new();

    // Initialize the prover before starting the server
    tracing::info!("Initializing prover...");

    // Load boundless config from file if provided, otherwise use default
    let mut boundless_config = boundless::BoundlessConfig::default();
    if let Some(config_file) = &args.config_file {
        let config_content = std::fs::read_to_string(config_file)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let file_config: boundless::BoundlessConfig = serde_json::from_str(&config_content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;
        boundless_config.merge(&file_config);
    }

    let prover_config = ProverConfig {
        offchain: args.offchain,
        pull_interval: args.pull_interval,
        rpc_url: args.rpc_url,
        boundless_config,
        url_ttl: args.url_ttl,
    };
    tracing::info!("Start with prover config: {:?}", prover_config);

    match state.init_prover(prover_config).await {
        Ok(_) => {
            tracing::info!("Prover initialized successfully");
        }
        Err(e) => {
            tracing::error!("Failed to initialize prover: {:?}", e);
            return Err(format!("Failed to initialize prover: {:?}", e).into());
        }
    }

    // Build router with max body size set to 10GB
    let app = Router::new()
        .route("/health", post(health_check))
        .route("/proof", post(proof_handler))
        .route("/status/:request_id", get(get_async_proof_status))
        .route("/requests", get(list_async_requests))
        .route("/prune", delete(delete_all_requests))
        .route("/stats", get(get_database_stats))
        .layer(DefaultBodyLimit::max(10000 * 1024 * 1024)) // max 10G
        .layer(cors)
        .with_state(state);

    let address = format!("{}:{}", args.address, args.port);
    // Start server
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!("Server listening on http://{}", &address);

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_deployment_type_parsing() {
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

        // Test invalid deployment type
        assert!(DeploymentType::from_str("invalid").is_err());
        assert!(DeploymentType::from_str("").is_err());
    }
}
