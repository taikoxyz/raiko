use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::OpenApi;

use crate::server::logging::{AsyncRequestLogger, RequestStats};
use raiko_reqactor::Actor;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyStats {
    pub api_key: String,
    pub request_count: u64,
    pub total_duration_ms: u64,
    pub average_duration_ms: f64,
    pub success_count: u64,
    pub error_count: u64,
    pub last_request_time: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestStatsResponse {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub average_response_time_ms: f64,
    pub api_key_stats: Vec<ApiKeyStats>,
}

#[utoipa::path(
    get,
    path = "/admin/apikey/stats",
    tag = "Admin",
    responses (
        (status = 200, description = "API key statistics", body = RequestStatsResponse),
    )
)]
/// Get API key request statistics
async fn get_apikey_stats(
    State(logger): State<Arc<AsyncRequestLogger>>,
) -> impl IntoResponse {
    let stats = logger.get_stats();
    
    let api_key_stats: Vec<ApiKeyStats> = stats
        .api_key_stats
        .iter()
        .map(|entry| {
            let key = entry.key();
            let value = entry.value();
            
            ApiKeyStats {
                api_key: key.clone(),
                request_count: value.request_count,
                total_duration_ms: value.total_duration_ms,
                average_duration_ms: value.average_duration_ms(),
                success_count: value.success_count,
                error_count: value.error_count,
                last_request_time: value.last_request_time,
            }
        })
        .collect();
    
    let response = RequestStatsResponse {
        total_requests: stats.total_requests,
        successful_requests: stats.successful_requests,
        failed_requests: stats.failed_requests,
        average_response_time_ms: stats.average_response_time_ms,
        api_key_stats,
    };
    
    (StatusCode::OK, Json(response))
}

#[derive(OpenApi)]
#[openapi(paths(get_apikey_stats))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new()
        .route("/stats", get(get_apikey_stats))
} 