use std::str::FromStr;

use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::interfaces::{AggregationOnlyRequest, ProofType};
use raiko_tasks::{TaskManager, TaskStatus};
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::{inc_guest_req_count, inc_host_req_count},
    server::api::v2::CancelStatus,
    Message, ProverState,
};

#[utoipa::path(post, path = "/proof/aggregate/cancel",
    tag = "Proving",
    request_body = AggregationOnlyRequest,
    responses (
        (status = 200, description = "Successfully cancelled proof aggregation task", body = CancelStatus)
    )
)]
#[debug_handler(state = ProverState)]
/// Cancel a proof aggregation task with requested config.
///
/// Accepts a proof aggregation request and cancels a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn cancel_handler(
    State(prover_state): State<ProverState>,
    Json(mut aggregation_request): Json<AggregationOnlyRequest>,
) -> HostResult<CancelStatus> {
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    aggregation_request.merge(&prover_state.request_config())?;

    let proof_type = ProofType::from_str(
        aggregation_request
            .proof_type
            .as_deref()
            .unwrap_or_default(),
    )?;
    inc_host_req_count(0);
    inc_guest_req_count(&proof_type, 0);

    if aggregation_request.proofs.is_empty() {
        return Err(anyhow::anyhow!("No proofs provided").into());
    }

    prover_state
        .task_channel
        .try_send(Message::CancelAggregate(aggregation_request.clone()))?;

    let mut manager = prover_state.task_manager();

    manager
        .update_aggregation_task_progress(&aggregation_request, TaskStatus::Cancelled, None)
        .await?;

    Ok(CancelStatus::Ok)
}

#[derive(OpenApi)]
#[openapi(paths(cancel_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", post(cancel_handler))
}
