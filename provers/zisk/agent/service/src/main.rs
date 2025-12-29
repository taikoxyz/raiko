use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

mod prover;
mod handlers;
mod types;

use prover::{ZiskProver, ZiskProverConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProofType {
    Batch,
    Aggregate,
}

#[derive(Debug, Deserialize)]
pub struct ProofRequest {
    pub input: Vec<u8>,
    pub proof_type: ProofType,
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ProofResponse {
    pub proof_data: Vec<u8>,
    pub proof_type: ProofType,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub prover: Arc<Mutex<ZiskProver>>,
}

impl AppState {
    fn new(config: ZiskProverConfig) -> Self {
        Self {
            prover: Arc::new(Mutex::new(ZiskProver::new(config))),
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "zisk-agent")]
#[command(about = "ZISK proof generation agent service")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "9998")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Concurrent processes for MPI
    #[arg(long)]
    concurrent_processes: Option<u32>,

    /// Threads per process
    #[arg(long)]
    threads_per_process: Option<u32>,

    /// Enable proof verification
    #[arg(long, default_value = "true")]
    verify: bool,
}

async fn health_check() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "service": "zisk-agent",
            "version": env!("CARGO_PKG_VERSION")
        })),
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("zisk_agent={},axum::routing={}", log_level, log_level).into())
        )
        .init();

    info!("Starting ZISK Agent v{}", env!("CARGO_PKG_VERSION"));
    info!("Listening on {}:{}", args.host, args.port);

    // Create prover configuration
    let prover_config = ZiskProverConfig {
        verify: args.verify,
        concurrent_processes: args.concurrent_processes,
        threads_per_process: args.threads_per_process,
    };

    // Create application state
    let state = AppState::new(prover_config);

    // Build the application router
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/proof", post(handlers::proof_handler))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024)) // 100MB max payload
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // Bind and serve
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    info!("ZISK Agent ready to serve requests");
    axum::serve(listener, app).await?;

    Ok(())
}