use axum::{debug_handler, http::StatusCode, routing::get, Router};
use utoipa::OpenApi;

use crate::ProverState;

#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses (
        (status = 200, description = "Proverd server is healthy"),
    )
)]
#[debug_handler(state = ProverState)]
/// Health check
async fn health() -> StatusCode {
    StatusCode::OK
}

#[derive(OpenApi)]
#[openapi(paths(health))]
struct HealthDocs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    HealthDocs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", get(health))
}
