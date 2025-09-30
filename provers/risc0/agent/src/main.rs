pub mod api;
pub mod boundless;
pub mod storage;
pub mod methods;

pub use boundless::{
    AgentError, AgentResult, AsyncProofRequest, DeploymentType, ElfType, 
    ProofRequestStatus, ProverConfig, BoundlessProver, ProofType as BoundlessProofType,
    generate_request_id,
};
pub use storage::{BoundlessStorage, DatabaseStats};

use axum::{
    extract::DefaultBodyLimit,
    Router,
    routing::{post, get, delete},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use utoipa_swagger_ui::SwaggerUi;
use utoipa_scalar::{Scalar, Servable};

use api::{
    handlers::*,
    create_docs,
};

#[derive(Debug, Clone)]
pub struct AppState {
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
    // #[arg(long, default_value = "https://ethereum-sepolia-rpc.publicnode.com")]
    #[arg(long, default_value = "https://base-rpc.publicnode.com")]
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

    // Generate OpenAPI documentation
    let docs = create_docs();

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/proof", post(proof_handler))
        .route("/status/:request_id", get(get_async_proof_status))
        .route("/requests", get(list_async_requests))
        .route("/prune", delete(delete_all_requests)) 
        .route("/stats", get(get_database_stats))
        // OpenAPI documentation endpoints
        .merge(SwaggerUi::new("/docs")
            .url("/api-docs/openapi.json", docs.clone()))
        .merge(Scalar::with_url("/scalar", docs.clone()))
        .route("/openapi.json", get(move || async move {
            axum::Json(docs)
        }))
        .layer(DefaultBodyLimit::max(10000 * 1024 * 1024))
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
