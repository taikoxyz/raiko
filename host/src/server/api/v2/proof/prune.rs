use axum::{debug_handler, extract::State, routing::post, Router};
use raiko_task_manager::TaskManager;
use utoipa::OpenApi;

use crate::{interfaces::HostResult, server::api::v2::PruneStatus, ProverState};

#[utoipa::path(post, path = "/proof/prune",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully pruned tasks", body = PruneStatus)
    )
)]
#[debug_handler(state = ProverState)]
/// Prune all tasks.
async fn prune_handler(State(prover_state): State<ProverState>) -> HostResult<PruneStatus> {
    let mut manager = prover_state.task_manager();

    manager.prune_db().await?;

    Ok(PruneStatus::Ok)
}

#[derive(OpenApi)]
#[openapi(paths(prune_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", post(prune_handler))
}
