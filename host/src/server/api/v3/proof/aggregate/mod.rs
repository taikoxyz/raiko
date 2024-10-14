use std::str::FromStr;

use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::interfaces::AggregationOnlyRequest;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::{TaskManager, TaskStatus};
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::api::v3::Status,
    server::HostError,
    Message, ProverState,
};

pub mod cancel;
pub mod prune;
pub mod report;

#[utoipa::path(post, path = "/proof/aggregate",
    tag = "Proving",
    request_body = AggregationRequest,
    responses (
        (status = 200, description = "Successfully submitted proof aggregation task, queried aggregation tasks in progress or retrieved aggregated proof.", body = Status)
    )
)]
#[debug_handler(state = ProverState)]
/// Submit a proof aggregation task with requested config, get task status or get proof value.
///
/// Accepts a proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn aggregation_handler(
    State(prover_state): State<ProverState>,
    Json(mut aggregation_request): Json<AggregationOnlyRequest>,
) -> HostResult<Status> {
    inc_current_req();
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    aggregation_request.merge(&prover_state.request_config())?;

    let proof_type = ProofType::from_str(
        aggregation_request
            .proof_type
            .as_deref()
            .unwrap_or_default(),
    )
    .map_err(HostError::Conversion)?;
    inc_host_req_count(0);
    inc_guest_req_count(&proof_type, 0);

    if aggregation_request.proofs.is_empty() {
        return Err(anyhow::anyhow!("No proofs provided").into());
    }

    let mut manager = prover_state.task_manager();

    let status = manager
        .get_aggregation_task_proving_status(&aggregation_request)
        .await?;

    let Some((latest_status, ..)) = status.0.last() else {
        // If there are no tasks with provided config, create a new one.
        manager
            .enqueue_aggregation_task(&aggregation_request)
            .await?;

        prover_state
            .task_channel
            .try_send(Message::Aggregate(aggregation_request))?;
        return Ok(Status::from(TaskStatus::Registered));
    };

    match latest_status {
        // If task has been cancelled add it to the queue again
        TaskStatus::Cancelled
        | TaskStatus::Cancelled_Aborted
        | TaskStatus::Cancelled_NeverStarted
        | TaskStatus::CancellationInProgress => {
            manager
                .update_aggregation_task_progress(
                    &aggregation_request,
                    TaskStatus::Registered,
                    None,
                )
                .await?;

            prover_state
                .task_channel
                .try_send(Message::Aggregate(aggregation_request))?;

            Ok(Status::from(TaskStatus::Registered))
        }
        // If the task has succeeded, return the proof.
        TaskStatus::Success => {
            let proof = manager
                .get_aggregation_task_proof(&aggregation_request)
                .await?;

            Ok(proof.into())
        }
        // For all other statuses just return the status.
        status => Ok(status.clone().into()),
    }
}

#[derive(OpenApi)]
#[openapi(paths(aggregation_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        cancel::create_docs(),
        report::create_docs(),
        prune::create_docs(),
    ]
    .into_iter()
    .fold(Docs::openapi(), |mut docs, curr| {
        docs.merge(curr);
        docs
    })
}

pub fn create_router() -> Router<ProverState> {
    Router::new()
        .route("/", post(aggregation_handler))
        .nest("/cancel", cancel::create_router())
        .nest("/prune", prune::create_router())
        .nest("/report", report::create_router())
}
