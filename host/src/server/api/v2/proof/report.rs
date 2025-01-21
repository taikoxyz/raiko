use crate::interfaces::HostResult;
use axum::{extract::State, routing::get, Json, Router};
use raiko_reqactor::Actor;
use raiko_reqpool::{RequestKey, Status, StatusWithContext};
use raiko_tasks::{
    AggregationTaskDescriptor, ProofTaskDescriptor, TaskDescriptor, TaskReport, TaskStatus,
};
use serde_json::Value;
use utoipa::OpenApi;

#[utoipa::path(post, path = "/proof/report",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully listed all current tasks", body = CancelStatus)
    )
)]
/// List all tasks.
///
/// Retrieve a list of `{ chain_id, blockhash, prover_type, prover, status }` items.
async fn report_handler(State(actor): State<Actor>) -> HostResult<Json<Value>> {
    let statuses = actor.pool_list_status().map_err(|e| anyhow::anyhow!(e))?;

    // For compatibility with the old API, we need to convert the statuses to the old format.
    let to_task_status = |status: StatusWithContext| match status.into_status() {
        Status::Registered => TaskStatus::Registered,
        Status::WorkInProgress => TaskStatus::WorkInProgress,
        Status::Cancelled => TaskStatus::Cancelled,
        Status::Success { .. } => TaskStatus::Success,
        Status::Failed { error } => TaskStatus::AnyhowError(error),
    };
    let to_task_descriptor = |request_key: RequestKey| match request_key {
        RequestKey::SingleProof(key) => TaskDescriptor::SingleProof(ProofTaskDescriptor {
            chain_id: *key.chain_id(),
            block_id: *key.block_number(),
            blockhash: *key.block_hash(),
            proof_system: *key.proof_type(),
            prover: key.prover_address().clone(),
        }),
        RequestKey::Aggregation(key) => TaskDescriptor::Aggregation(AggregationTaskDescriptor {
            aggregation_ids: key.block_numbers().clone(),
            proof_type: Some(key.proof_type().to_string()),
        }),
    };

    let task_report: Vec<TaskReport> = statuses
        .into_iter()
        .map(|(request_key, status)| (to_task_descriptor(request_key), to_task_status(status)))
        .collect();
    Ok(Json(serde_json::to_value(task_report)?))
}

#[derive(OpenApi)]
#[openapi(paths(report_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", get(report_handler))
}
