use axum::{debug_handler, extract::State, routing::get, Json, Router};
use raiko_tasks::TaskManager;
use serde_json::Value;
use utoipa::OpenApi;

use crate::{interfaces::HostResult, ProverState};

#[utoipa::path(post, path = "/proof/list",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully listed all proofs & Ids", body = CancelStatus)
    )
)]
#[debug_handler(state = ProverState)]
/// List all tasks.
///
/// Retrieve a list of `{ chain_id, blockhash, prover_type, prover, status }` items.
async fn list_handler(State(prover_state): State<ProverState>) -> HostResult<Json<Value>> {
    let mut manager = prover_state.task_manager();

    let ids = manager.list_stored_ids().await?;

    Ok(Json(serde_json::to_value(ids)?))
}

#[derive(OpenApi)]
#[openapi(paths(list_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", get(list_handler))
}
