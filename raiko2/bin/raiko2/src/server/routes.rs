//! API route definitions.

use axum::{
    routing::{get, post},
    Router,
};

use super::handlers;
use super::state::AppState;

/// Build API routes.
pub fn api_routes() -> Router<AppState> {
    Router::new()
        // Health check
        .route("/health", get(handlers::health))
        // API v2 routes
        .route("/v2/proof/batch", post(handlers::request_batch_proof))
        .route("/v2/proof/:id", get(handlers::get_proof_status))
        .route("/v2/proof/:id/cancel", post(handlers::cancel_proof))
        // Info routes
        .route("/v2/info", get(handlers::get_info))
}
