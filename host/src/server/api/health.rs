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
///
/// Currently only responds with an OK status.
/// Will return more detailed status information soon.
async fn handler() -> StatusCode {
    StatusCode::OK
}

#[derive(OpenApi)]
#[openapi(paths(handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", get(handler))
}
