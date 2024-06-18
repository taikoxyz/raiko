use axum::{
    debug_handler,
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use raiko_task_manager::{TaskDb, TaskProvingStatus};
use utoipa::OpenApi;

use crate::{interfaces::HostResult, ProverState};

#[utoipa::path(get, path = "/proof/status/:task_id",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully retrieved proving task status", body = Status)
    )
)]
#[debug_handler(state = ProverState)]
/// Check for a proving task status.
///
/// Accepts a proving task id.
async fn status_handler(
    State(prover_state): State<ProverState>,
    Path(task_id): Path<u64>,
) -> HostResult<Json<TaskProvingStatus>> {
    let db = TaskDb::open_or_create(&prover_state.opts.sqlite_file)?;
    let mut manager = db.manage()?;
    let status = manager.get_task_proving_status_by_id(task_id)?;
    Ok(Json(status))
}

#[derive(OpenApi)]
#[openapi(paths(status_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/status/:task_id", get(status_handler))
}
