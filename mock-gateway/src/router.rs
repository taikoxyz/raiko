use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;

use crate::{generated::handle_shasta_request, AppState};

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v3/proof/batch/shasta", post(mock_shasta_handler))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn mock_shasta_handler(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let ctx = state.new_context();
    Json(handle_shasta_request(&ctx, &body))
}
