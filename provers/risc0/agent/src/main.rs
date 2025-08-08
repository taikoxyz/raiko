pub mod boundless;
pub use boundless::{
    AgentError, AgentResult, DeploymentType, ElfType, ProverConfig, Risc0BoundlessProver,
};

pub mod methods;

use axum::{
    Json,
    extract::{DefaultBodyLimit, State},
};
use axum::{Router, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ProofType {
    Batch,
    Aggregate,
    Update(ElfType),
}

#[derive(Debug, Deserialize)]
struct ProofRequest {
    input: Vec<u8>,
    proof_type: ProofType,
    elf: Option<Vec<u8>>,
    config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ProofResponse {
    proof_data: Vec<u8>,
    proof_type: ProofType,
    success: bool,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct AppState {
    prover: Arc<Mutex<Option<Risc0BoundlessProver>>>,
    prover_init_time: Arc<Mutex<Option<std::time::Instant>>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            prover: Arc::new(Mutex::new(None)),
            prover_init_time: Arc::new(Mutex::new(None)),
        }
    }

    async fn init_prover(&self, config: ProverConfig) -> AgentResult<Risc0BoundlessProver> {
        let prover = Risc0BoundlessProver::new(config).await.map_err(|e| {
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
    async fn get_or_refresh_prover(&self) -> AgentResult<Risc0BoundlessProver> {
        let mut prover_guard = self.prover.lock().await;
        let config_guard = prover_guard.as_ref().unwrap().prover_config();
        let mut time_guard = self.prover_init_time.lock().await;
        let now = std::time::Instant::now();
        let ttl = std::time::Duration::from_secs(3000);

        let should_refresh = match *time_guard {
            Some(init_time) => now.duration_since(init_time) > ttl,
            None => true,
        };

        if should_refresh || prover_guard.is_none() {
            tracing::info!("Prover TTL exceeded or not initialized, re-initializing prover...");
            let prover = Risc0BoundlessProver::new(config_guard).await.map_err(|e| {
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

async fn proof_handler(
    State(state): State<AppState>,
    Json(request): Json<ProofRequest>,
) -> (StatusCode, Json<ProofResponse>) {
    tracing::info!(
        "Received proof generation request size: {:?}",
        request.input.len()
    );

    // Get the initialized prover
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(e) => {
            tracing::error!("Failed to get or refresh prover: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProofResponse {
                    proof_data: vec![],
                    proof_type: request.proof_type,
                    success: false,
                    error: Some(format!("Failed to get or refresh prover: {}", e)),
                }),
            );
        }
    };

    // Use empty output if not provided
    let output_data = vec![];
    let config = request
        .config
        .unwrap_or_else(|| serde_json::Value::default());

    tracing::info!("Running proof generation...");

    // Generate proof with timeout
    let proof_result = tokio::time::timeout(
        std::time::Duration::from_secs(9600), // 3 hour timeout
        async {
            match request.proof_type.clone() {
                ProofType::Batch => prover
                    .batch_run(request.input, &output_data, &config)
                    .await
                    .map_err(|e| format!("Failed to run batch proof: {}", e)),
                ProofType::Aggregate => prover
                    .aggregate(request.input, &output_data, &config)
                    .await
                    .map_err(|e| format!("Failed to run aggregation proof: {}", e)),
                ProofType::Update(elf_type) => prover
                    .update(request.elf.unwrap(), elf_type)
                    .await
                    .map_err(|e| format!("Failed to run update proof: {}", e)),
            }
        },
    )
    .await;

    match proof_result {
        Ok(Ok(proof_data)) => {
            tracing::info!("Proof generated successfully");
            (
                StatusCode::OK,
                Json(ProofResponse {
                    proof_data,
                    proof_type: request.proof_type,
                    success: true,
                    error: None,
                }),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("Proof generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProofResponse {
                    proof_data: vec![],
                    proof_type: request.proof_type,
                    success: false,
                    error: Some(e),
                }),
            )
        }
        Err(_) => {
            tracing::error!("Proof generation timed out");
            (
                StatusCode::REQUEST_TIMEOUT,
                Json(ProofResponse {
                    proof_data: vec![],
                    proof_type: request.proof_type,
                    success: false,
                    error: Some("Proof generation timed out after 2 hour".to_string()),
                }),
            )
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
