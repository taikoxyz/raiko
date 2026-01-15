use crate::interfaces::HostResult;
use axum::{extract::State, routing::get, Json, Router};
use raiko_reqactor::Actor;
use raiko_reqpool::{RequestKey, Status, StatusWithContext};
use raiko_tasks::{
    AggregationTaskDescriptor, BatchGuestInputTaskDescriptor, BatchProofTaskDescriptor,
    GuestInputTaskDescriptor, ProofTaskDescriptor, ShastaGuestInputTaskDescriptor,
    ShastaProofTaskDescriptor, TaskDescriptor, TaskReport, TaskStatus,
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
    let statuses = actor
        .pool_list_status()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    // For compatibility with the old API, we need to convert the statuses to the old format.
    let to_task_status = |status: StatusWithContext| match status.into_status() {
        Status::Registered => TaskStatus::Registered,
        Status::WorkInProgress => TaskStatus::WorkInProgress,
        Status::Cancelled => TaskStatus::Cancelled,
        Status::Success { .. } => TaskStatus::Success,
        Status::Failed { error } => TaskStatus::AnyhowError(error),
    };
    let to_task_descriptor = |request_key: RequestKey| match request_key {
        RequestKey::GuestInput(key) => TaskDescriptor::GuestInput(GuestInputTaskDescriptor {
            chain_id: *key.chain_id(),
            block_id: *key.block_number(),
            blockhash: *key.block_hash(),
        }),
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
        RequestKey::BatchProof(key) => TaskDescriptor::BatchProof(BatchProofTaskDescriptor {
            chain_id: *key.guest_input_key().chain_id(),
            batch_id: *key.guest_input_key().batch_id(),
            l1_height: *key.guest_input_key().l1_inclusion_height(),
            proof_system: *key.proof_type(),
            prover: key.prover_address().clone(),
        }),
        RequestKey::BatchGuestInput(key) => {
            TaskDescriptor::BatchGuestInput(BatchGuestInputTaskDescriptor {
                chain_id: *key.chain_id(),
                batch_id: *key.batch_id(),
                l1_height: *key.l1_inclusion_height(),
            })
        }
        RequestKey::ShastaGuestInput(key) => {
            TaskDescriptor::ShastaGuestInput(ShastaGuestInputTaskDescriptor {
                proposal_id: *key.proposal_id(),
                l1_network: key.l1_network().clone(),
                l2_network: key.l2_network().clone(),
            })
        }
        RequestKey::ShastaProof(key) => TaskDescriptor::ShastaProof(ShastaProofTaskDescriptor {
            proposal_id: *key.guest_input_key().proposal_id(),
            l1_network: key.guest_input_key().l1_network().clone(),
            l2_network: key.guest_input_key().l2_network().clone(),
            proof_system: *key.proof_type(),
            prover: key.actual_prover_address().clone(),
        }),
        RequestKey::ShastaAggregation(key) => {
            TaskDescriptor::Aggregation(AggregationTaskDescriptor {
                aggregation_ids: key.block_numbers().clone(),
                proof_type: Some(key.proof_type().to_string()),
            })
        }
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
