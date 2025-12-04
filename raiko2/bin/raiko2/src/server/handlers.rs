//! HTTP request handlers.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::state::AppState;

/// Health check response.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// Health check endpoint.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Server info response.
#[derive(Serialize)]
pub struct InfoResponse {
    pub version: &'static str,
    pub prover: String,
    pub supported_provers: Vec<&'static str>,
}

/// Get server info.
pub async fn get_info(State(state): State<AppState>) -> Json<InfoResponse> {
    Json(InfoResponse {
        version: env!("CARGO_PKG_VERSION"),
        prover: format!("{:?}", state.config.prover.prover_type),
        supported_provers: vec!["risc0", "sp1"],
    })
}

/// Batch proof request.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Fields will be used when proof generation is implemented
pub struct BatchProofRequest {
    pub batch_id: u64,
    pub l1_inclusion_block: u64,
    #[serde(default)]
    pub prover_type: Option<String>,
    #[serde(default)]
    pub blob_proof_type: Option<String>,
    #[serde(default)]
    pub prover: Option<String>,
    #[serde(default)]
    pub graffiti: Option<String>,
}

/// Proof response.
#[derive(Serialize)]
pub struct ProofResponse {
    pub id: String,
    pub status: ProofStatus,
}

/// Proof status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProofStatus {
    Pending,
    Proving,
    Completed,
    Failed,
    Cancelled,
}

/// Request a batch proof.
pub async fn request_batch_proof(
    State(_state): State<AppState>,
    Json(req): Json<BatchProofRequest>,
) -> Result<Json<ProofResponse>, ApiError> {
    info!(
        "Received batch proof request: batch_id={}, l1_block={}",
        req.batch_id, req.l1_inclusion_block
    );

    // TODO: Implement actual proof generation
    // For now, return a placeholder response

    let proof_id = format!(
        "proof-{}-{}-{}",
        req.batch_id,
        req.l1_inclusion_block,
        chrono_lite_timestamp()
    );

    Ok(Json(ProofResponse {
        id: proof_id,
        status: ProofStatus::Pending,
    }))
}

/// Get proof status.
pub async fn get_proof_status(
    State(_state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProofStatusResponse>, ApiError> {
    info!("Getting proof status for: {}", id);

    // TODO: Implement actual status lookup
    Ok(Json(ProofStatusResponse {
        id,
        status: ProofStatus::Pending,
        proof: None,
        error: None,
    }))
}

/// Proof status response.
#[derive(Serialize)]
pub struct ProofStatusResponse {
    pub id: String,
    pub status: ProofStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Cancel proof request.
pub async fn cancel_proof(
    State(_state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProofStatusResponse>, ApiError> {
    info!("Cancelling proof: {}", id);

    // TODO: Implement actual cancellation
    Ok(Json(ProofStatusResponse {
        id,
        status: ProofStatus::Cancelled,
        proof: None,
        error: None,
    }))
}

/// API error type.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({
            "error": self.message,
        });
        (self.status, Json(body)).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }
}

/// Simple timestamp for proof IDs (no external dependency).
fn chrono_lite_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
