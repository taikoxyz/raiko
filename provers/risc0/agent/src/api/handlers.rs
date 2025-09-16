use axum::{
    Json,
    extract::{State, Path},
    http::StatusCode,
};
use utoipa;
use alloy_primitives_v1p2p0::U256;

use crate::{
    AppState, AgentError, AsyncProofRequest, ProofRequestStatus,
    BoundlessProofType as BoundlessProofType, generate_request_id,
};
use super::types::{
    AsyncProofRequestData, AsyncProofResponse, DetailedStatusResponse, 
    RequestListResponse, HealthResponse, DatabaseStatsResponse, DeleteAllResponse,
    ErrorResponse, ProofType
};

/// Convert internal ProofRequestStatus to user-friendly API response
fn map_status_to_api_response(request: &AsyncProofRequest) -> DetailedStatusResponse {
    let (status, status_message, proof_data, error) = match &request.status {
        ProofRequestStatus::Preparing => (
            "preparing".to_string(),
            "Request received. Executing guest program and preparing for market submission...".to_string(),
            None,
            None,
        ),
        ProofRequestStatus::Submitted { .. } => (
            "submitted".to_string(),
            "The proof request has been submitted to the boundless market and is waiting for an available prover to pick it up.".to_string(),
            None,
            None,
        ),
        ProofRequestStatus::Locked { prover, .. } => (
            "in_progress".to_string(),
            format!("A prover {} has accepted the request and is generating the proof.", 
                prover.as_ref().map(|p| format!("({})", p)).unwrap_or_else(|| "".to_string())),
            None,
            None,
        ),
        ProofRequestStatus::Fulfilled { proof, .. } => (
            "completed".to_string(),
            "The proof has been successfully generated and is ready for download.".to_string(),
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

    // Extract market_request_id from the status enum when available
    let market_request_id = match &request.status {
        ProofRequestStatus::Submitted { market_request_id } => *market_request_id,
        ProofRequestStatus::Locked { market_request_id, .. } => *market_request_id,
        ProofRequestStatus::Fulfilled { market_request_id, .. } => *market_request_id,
        _ => request.market_request_id,
    };

    DetailedStatusResponse {
        request_id: request.request_id.clone(),
        market_request_id,
        status,
        status_message,
        proof_data,
        error,
    }
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse, 
         example = json!({
             "status": "healthy",
             "service": "boundless-agent"
         }))
    )
)]
/// Health check endpoint
pub async fn health_check() -> (StatusCode, Json<HealthResponse>) {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "healthy".to_string(),
            service: "boundless-agent".to_string(),
        }),
    )
}

#[utoipa::path(
    post,
    path = "/proof",
    tag = "Proof",
    request_body = AsyncProofRequestData,
    responses(
        (status = 202, description = "Proof request accepted", body = AsyncProofResponse,
         example = json!({
             "request_id": "req_abc123def456",
             "market_request_id": "0",
             "status": "preparing",
             "message": "Proof request received and preparing for market submission"
         })),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
/// Submit an asynchronous proof generation request
pub async fn proof_handler(
    State(state): State<AppState>,
    Json(request): Json<AsyncProofRequestData>,
) -> Result<(StatusCode, Json<AsyncProofResponse>), (StatusCode, Json<ErrorResponse>)> {
    if request.input.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "ValidationError".to_string(),
                message: "Input data cannot be empty".to_string(),
            }),
        ));
    }
    
    // Validate ELF data for Update proof type
    if let ProofType::Update(_) = &request.proof_type {
        match &request.elf {
            None => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "ValidationError".to_string(),
                        message: "ELF data is required for Update proof type".to_string(),
                    }),
                ));
            }
            Some(elf_data) if elf_data.is_empty() => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "ValidationError".to_string(),
                        message: "ELF data cannot be empty for Update proof type".to_string(),
                    }),
                ));
            }
            _ => {} // ELF data is valid
        }
    }
    
    // Convert ProofType to BoundlessProofType for request ID generation
    let boundless_proof_type = match &request.proof_type {
        ProofType::Batch => BoundlessProofType::Batch,
        ProofType::Aggregate => BoundlessProofType::Aggregate,
        ProofType::Update(elf_type) => BoundlessProofType::Update(elf_type.clone()),
    };
    
    // Generate deterministic request_id
    let request_id = generate_request_id(&request.input, &boundless_proof_type);

    tracing::info!(
        "Received proof submission: {} (size: {} bytes)",
        request_id,
        request.input.len()
    );

    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(e) => {
            tracing::error!("Failed to get prover: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "ProverInitializationError".to_string(),
                    message: "Failed to initialize prover".to_string(),
                }),
            ));
        }
    };

    let config = request.config.unwrap_or_else(|| serde_json::Value::default());
    
    // Convert ProofType to BoundlessProofType and call appropriate async method
    let result = match request.proof_type {
        ProofType::Batch => {
            prover.batch_run(request_id.clone(), request.input, &config).await
        }
        ProofType::Aggregate => {
            prover.aggregate(request_id.clone(), request.input, &config).await
        }
        ProofType::Update(elf_type) => {
            // ELF data validation was already done above, so it should be safe to extract
            match request.elf {
                Some(elf_data) => prover.update(request_id.clone(), elf_data, elf_type).await,
                None => Err(AgentError::RequestBuildError("ELF data validation failed".to_string()))
            }
        }
    };
    
    match result {
        Ok(returned_request_id) => {
            tracing::info!("Proof request received and preparing: {}", returned_request_id);
            Ok((
                StatusCode::ACCEPTED,
                Json(AsyncProofResponse {
                    request_id: returned_request_id,
                    market_request_id: U256::ZERO,
                    status: "preparing".to_string(),
                    message: "Proof request received and preparing for market submission".to_string(),
                }),
            ))
        }
        Err(e) => {
            tracing::error!("Failed to submit proof: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "ProofSubmissionError".to_string(),
                    message: "Failed to submit proof request".to_string(),
                }),
            ))
        }
    }
}

#[utoipa::path(
    get,
    path = "/status/{request_id}",
    tag = "Status",
    params(
        ("request_id" = String, Path, description = "Unique request identifier")
    ),
    responses(
        (status = 200, description = "Request status retrieved", body = DetailedStatusResponse,
         example = json!({
             "request_id": "req_abc123def456",
             "market_request_id": "123456789",
             "status": "in_progress",
             "status_message": "A prover has accepted the request and is generating the proof",
             "proof_data": null,
             "error": null
         })),
        (status = 404, description = "Request not found", body = ErrorResponse,
         example = json!({
             "error": "Request not found",
             "message": "No proof request found with the specified request_id"
         })),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
/// Get the current status of a proof request
pub async fn get_async_proof_status(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<Json<DetailedStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(_e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "ProverInitializationError".to_string(),
                    message: "Failed to initialize prover".to_string(),
                }),
            ));
        }
    };

    match prover.get_request_status(&request_id).await {
        Some(request) => {
            let detailed_response = map_status_to_api_response(&request);
            Ok(Json(detailed_response))
        },
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "RequestNotFound".to_string(),
                message: "No proof request found with the specified request_id".to_string(),
            }),
        )),
    }
}

#[utoipa::path(
    get,
    path = "/requests",
    tag = "Status",
    responses(
        (status = 200, description = "List of active requests", body = RequestListResponse,
         example = json!({
             "active_requests": 2,
             "requests": [
                 {
                     "request_id": "req_abc123def456",
                     "market_request_id": "123456789",
                     "status": "in_progress",
                     "status_message": "A prover has accepted the request and is generating the proof",
                     "proof_data": null,
                     "error": null
                 }
             ]
         })),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
/// List all active proof requests
pub async fn list_async_requests(
    State(state): State<AppState>,
) -> Result<Json<RequestListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(_e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "ProverInitializationError".to_string(),
                    message: "Failed to initialize prover".to_string(),
                }),
            ));
        }
    };

    let requests = prover.list_active_requests().await;
    let detailed_requests: Vec<DetailedStatusResponse> = requests
        .iter()
        .map(|req| map_status_to_api_response(req))
        .collect();
    
    Ok(Json(RequestListResponse {
        active_requests: requests.len(),
        requests: detailed_requests,
    }))
}

#[utoipa::path(
    get,
    path = "/db/stats",
    tag = "Maintenance",
    responses(
        (status = 200, description = "Database statistics", body = DatabaseStatsResponse,
         example = json!({
             "database_stats": {
                 "total_requests": 1247,
                 "active_requests": 3,
                 "completed_requests": 1200,
                 "failed_requests": 44
             }
         })),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
/// Get database statistics for monitoring
pub async fn get_database_stats(
    State(state): State<AppState>,
) -> Result<Json<DatabaseStatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(_e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "ProverInitializationError".to_string(),
                    message: "Failed to initialize prover".to_string(),
                }),
            ));
        }
    };

    match prover.get_database_stats().await {
        Ok(stats) => Ok(Json(DatabaseStatsResponse {
            database_stats: stats,
        })),
        Err(_e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "DatabaseError".to_string(),
                message: "Failed to retrieve database statistics".to_string(),
            }),
        )),
    }
}

#[utoipa::path(
    delete,
    path = "/requests",
    tag = "Maintenance",
    responses(
        (status = 200, description = "All requests deleted", body = DeleteAllResponse,
         example = json!({
             "message": "Successfully deleted 1247 requests",
             "deleted_count": 1247
         })),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
/// Delete all requests from the database
pub async fn delete_all_requests(
    State(state): State<AppState>,
) -> Result<Json<DeleteAllResponse>, (StatusCode, Json<ErrorResponse>)> {
    let prover = match state.get_or_refresh_prover().await {
        Ok(prover) => prover,
        Err(_e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "ProverInitializationError".to_string(),
                    message: "Failed to initialize prover".to_string(),
                }),
            ));
        }
    };

    match prover.delete_all_requests().await {
        Ok(deleted_count) => Ok(Json(DeleteAllResponse {
            message: format!("Successfully deleted {} requests", deleted_count),
            deleted_count,
        })),
        Err(_e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "DatabaseError".to_string(),
                message: "Failed to delete requests from database".to_string(),
            }),
        )),
    }
}