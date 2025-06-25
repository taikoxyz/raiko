pub mod boundless;
pub use boundless::{AgentError, AgentResult, Risc0BoundlessProver};

pub mod methods;

use axum::{Router, extract::State, http::StatusCode, response::Json, routing::post};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ProofType {
    Batch,
    Agg,
}

#[derive(Debug, Deserialize)]
struct ProofRequest {
    input: Vec<u8>,
    proof_type: ProofType,
    elf_path: Option<String>,
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
}

impl AppState {
    fn new() -> Self {
        Self {
            prover: Arc::new(Mutex::new(None)),
        }
    }

    async fn init_prover(&self) -> AgentResult<Risc0BoundlessProver> {
        let prover = Risc0BoundlessProver::init_prover()
            .await
            .map_err(|e| AgentError::AgentError(format!("Failed to initialize prover: {}", e)))?;
        self.prover.lock().await.replace(prover.clone());
        Ok(prover)
    }
}

async fn proof_handler(
    State(state): State<AppState>,
    Json(request): Json<ProofRequest>,
) -> (StatusCode, Json<ProofResponse>) {
    tracing::info!(
        "Received proof generation request: {:?}",
        request.proof_type
    );

    // Get the initialized prover
    let prover = {
        let prover_guard = state.prover.lock().await;
        prover_guard.as_ref().unwrap().clone()
    };

    // Use empty output if not provided
    let output_data = vec![];
    let config = request
        .config
        .unwrap_or_else(|| serde_json::Value::default());

    tracing::info!("Running proof generation...");

    // Generate proof with timeout
    let proof_result = tokio::time::timeout(
        std::time::Duration::from_secs(3600), // 1 hour timeout
        async {
            match request.proof_type {
                ProofType::Batch => prover
                    .batch_run(request.input, &output_data, &config)
                    .await
                    .map_err(|e| format!("Failed to run batch proof: {}", e)),
                ProofType::Agg => prover
                    .aggregate(request.input, &output_data, &config)
                    .await
                    .map_err(|e| format!("Failed to run aggregation proof: {}", e)),
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
                    error: Some("Proof generation timed out after 1 hour".to_string()),
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    tracing::info!("Starting RISC0 Boundless Agent Web Service...");

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Create app state
    let state = AppState::new();

    // Initialize the prover before starting the server
    tracing::info!("Initializing prover...");
    match state.init_prover().await {
        Ok(_) => {
            tracing::info!("Prover initialized successfully");
        }
        Err(e) => {
            tracing::error!("Failed to initialize prover: {:?}", e);
            return Err(format!("Failed to initialize prover: {:?}", e).into());
        }
    }

    // Build router
    let app = Router::new()
        .route("/health", post(health_check))
        .route("/proof", post(proof_handler))
        .layer(cors)
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:9999").await?;
    tracing::info!("Server listening on http://0.0.0.0:9999");

    axum::serve(listener, app).await?;

    Ok(())
}
