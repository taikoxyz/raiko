use axum::{http::StatusCode, routing::get, Router};
use utoipa::OpenApi;

use raiko_reqactor::Gateway;

#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses (
        (status = 200, description = "Proverd server is healthy"),
    )
)]
// #[debug_handler(state = Gateway)]
/// Health check
///
/// Currently only responds with an OK status.
/// Will return more detailed status information soon.
async fn health_handler() -> StatusCode {
    StatusCode::OK
}

#[derive(OpenApi)]
#[openapi(paths(health_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router<P: raiko_reqpool::Pool + 'static>() -> Router<Gateway<P>> {
    Router::new().route("/", get(health_handler))
}
