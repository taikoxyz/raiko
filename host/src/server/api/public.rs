use crate::server::api::v1::{health, metrics};
use axum::Router;
use raiko_reqactor::Actor;

/// used for health check and metrics
pub fn public_routes() -> Router<Actor> {
    Router::new()
        .nest("/", health::create_router())
        .nest("/healthz", health::create_router())
        .nest("/health", health::create_router())
        .nest("/metrics", metrics::create_router())
}
