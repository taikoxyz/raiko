use axum::{extract::State, http::StatusCode, Json};
use tracing::{info, error};

use crate::{AppState, ProofRequest, ProofResponse, ProofType};

pub async fn proof_handler(
    State(state): State<AppState>,
    Json(request): Json<ProofRequest>,
) -> (StatusCode, Json<ProofResponse>) {
    info!(
        "Received proof generation request: type={:?}, input_size={}",
        request.proof_type, request.input.len()
    );

    let prover = state.prover.lock().await;

    // Generate proof with timeout
    let proof_result = tokio::time::timeout(
        std::time::Duration::from_secs(3600), // 1 hour timeout
        async {
            match request.proof_type {
                ProofType::Batch => {
                    info!("Starting batch proof generation");
                    prover.batch_proof(request.input).await
                }
                ProofType::Aggregate => {
                    info!("Starting aggregation proof generation");
                    prover.aggregation_proof(request.input).await
                }
            }
        },
    )
    .await;

    match proof_result {
        Ok(Ok(zisk_response)) => {
            info!("Proof generated successfully");
            
            // Serialize the ZISK response
            let proof_data = match bincode::serialize(&zisk_response) {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to serialize proof response: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ProofResponse {
                            proof_data: vec![],
                            proof_type: request.proof_type,
                            success: false,
                            error: Some(format!("Failed to serialize response: {}", e)),
                        }),
                    );
                }
            };

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
            error!("Proof generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProofResponse {
                    proof_data: vec![],
                    proof_type: request.proof_type,
                    success: false,
                    error: Some(e.to_string()),
                }),
            )
        }
        Err(_) => {
            error!("Proof generation timed out");
            (
                StatusCode::REQUEST_TIMEOUT,
                Json(ProofResponse {
                    proof_data: vec![],
                    proof_type: request.proof_type,
                    success: false,
                    error: Some("Proof generation timed out after 2 hours".to_string()),
                }),
            )
        }
    }
}