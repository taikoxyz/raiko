use crate::{
    interfaces::{HostError, HostResult},
    server::api::v2::CancelStatus,
    Message, ProverState,
};
use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::interfaces::AggregationOnlyRequest;
use raiko_tasks::{TaskManager, TaskStatus};
use utoipa::OpenApi;

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

    let status = prover_state
        .task_manager()
        .get_aggregation_task_proving_status(&aggregation_request)
        .await?;

    let Some((latest_status, ..)) = status.0.last() else {
        return Err(HostError::Io(std::io::ErrorKind::NotFound.into()));
    };

    let mut should_signal_cancel = false;
    let returning_cancel_status = match latest_status {
        /* Task is already cancelled, so we don't need further action */
        TaskStatus::Cancelled
        | TaskStatus::Cancelled_Aborted
        | TaskStatus::Cancelled_NeverStarted
        | TaskStatus::CancellationInProgress => CancelStatus::Ok,

        /* Task is not completed, so we need to signal the prover to cancel */
        TaskStatus::Registered | TaskStatus::WorkInProgress => {
            should_signal_cancel = true;
            CancelStatus::Ok
        }

        /* Task is completed with failure, so we don't need further action, but in case of
         * retry we safe to signal the prover to cancel */
        TaskStatus::ProofFailure_Generic
        | TaskStatus::ProofFailure_OutOfMemory
        | TaskStatus::NetworkFailure(_)
        | TaskStatus::IoFailure(_)
        | TaskStatus::AnyhowError(_)
        | TaskStatus::GuestProverFailure(_)
        | TaskStatus::InvalidOrUnsupportedBlock
        | TaskStatus::UnspecifiedFailureReason
        | TaskStatus::TaskDbCorruption(_)
        | TaskStatus::SystemPaused => {
            should_signal_cancel = true;
            CancelStatus::Error {
                error: "Task already completed".to_string(),
                message: format!("Task already completed, status: {:?}", latest_status),
            }
        }

        /* Task is completed with success, so we return an error */
        TaskStatus::Success => CancelStatus::Error {
            error: "Task already completed".to_string(),
            message: format!("Task already completed, status: {:?}", latest_status),
        },
    };

    if should_signal_cancel {
        prover_state
            .task_channel
            .try_send(Message::CancelAggregate(aggregation_request.clone()))?;

        let mut manager = prover_state.task_manager();

        manager
            .update_aggregation_task_progress(&aggregation_request, TaskStatus::Cancelled, None)
            .await?;
    }

    Ok(returning_cancel_status)
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
