use axum::{response::IntoResponse, routing::get, Router};
use raiko_reqactor::Actor;

/// used for health check and TODO: metrics
pub fn public_routes() -> Router<Actor> {
    Router::new()
        .route("/", get(healthz))
        .route("/healthz", get(healthz))
}

// healthz handler
async fn healthz() -> impl IntoResponse {
    axum::http::StatusCode::OK
}
