use axum::{debug_handler, extract::State, routing::get, Json, Router};
use raiko_tasks::{AggregationTaskReport, TaskManager};
use utoipa::OpenApi;

use crate::{interfaces::HostResult, ProverState};

#[utoipa::path(post, path = "/proof/aggregate/report",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully retrieved a report of all aggregation tasks", body = AggregationTaskReport)
    )
)]
#[debug_handler(state = ProverState)]
/// List all aggregation tasks.
///
/// Retrieve a list of aggregation task reports.
async fn report_handler(
    State(prover_state): State<ProverState>,
) -> HostResult<Json<Vec<AggregationTaskReport>>> {
    let mut manager = prover_state.task_manager();

    let task_report = manager.list_all_aggregation_tasks().await?;

    Ok(Json(task_report))
}

#[derive(OpenApi)]
#[openapi(paths(report_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", get(report_handler))
}
