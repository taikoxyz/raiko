use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::{interfaces::ProofRequest, provider::get_task_data};
use raiko_task_manager::{
    get_task_manager, EnqueueTaskParams, TaskManager, TaskProvingStatus, TaskStatus,
};
use serde_json::Value;
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::api::v1::ProofResponse,
    ProverState,
};

mod cancel;

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully submitted proof task", body = Status)
    )
)]
#[debug_handler(state = ProverState)]
/// Submit a proof task with requested config, get task status or get proof value.
///
/// Accepts a proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn proof_handler(
    State(prover_state): State<ProverState>,
    Json(req): Json<Value>,
) -> HostResult<Json<Value>> {
    inc_current_req();
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = prover_state.opts.proof_request_opt.clone();
    config.merge(&req)?;

    // Construct the actual proof request from the available configs.
    let proof_request = ProofRequest::try_from(config)?;
    inc_host_req_count(proof_request.block_number);
    inc_guest_req_count(&proof_request.proof_type, proof_request.block_number);

    let (chain_id, block_hash) = get_task_data(
        &proof_request.network,
        proof_request.block_number,
        &prover_state.chain_specs,
    )
    .await?;

    let mut manager = get_task_manager(&(&prover_state.opts).into());
    let status = manager
        .get_task_proving_status(
            chain_id,
            block_hash,
            proof_request.proof_type,
            Some(proof_request.prover.to_string()),
        )
        .await?;

    let Some(TaskProvingStatus(latest_status, ..)) = status.last() else {
        // If there are no tasks with provided config, create a new one.
        manager
            .enqueue_task(&EnqueueTaskParams {
                chain_id,
                blockhash: block_hash,
                proof_type: proof_request.proof_type,
                prover: proof_request.prover.to_string(),
                block_number: proof_request.block_number,
            })
            .await?;

        prover_state.task_channel.try_send((
            proof_request.clone(),
            prover_state.opts,
            prover_state.chain_specs,
        ))?;

        return Ok(status_into_response(TaskStatus::Registered));
    };

    match latest_status {
        // If task has been cancelled add it to the queue again
        TaskStatus::Cancelled
        | TaskStatus::Cancelled_Aborted
        | TaskStatus::Cancelled_NeverStarted
        | TaskStatus::CancellationInProgress => {
            manager
                .enqueue_task(&EnqueueTaskParams {
                    chain_id,
                    blockhash: block_hash,
                    proof_type: proof_request.proof_type,
                    prover: proof_request.prover.to_string(),
                    block_number: proof_request.block_number,
                })
                .await?;

            prover_state.task_channel.try_send((
                proof_request.clone(),
                prover_state.opts,
                prover_state.chain_specs,
            ))?;

            Ok(status_into_response(TaskStatus::Registered))
        }
        // If the task has succeeded, return the proof.
        TaskStatus::Success => {
            let proof = manager
                .get_task_proof(
                    chain_id,
                    block_hash,
                    proof_request.proof_type,
                    Some(proof_request.prover.to_string()),
                )
                .await?;

            Ok(Json(
                ProofResponse {
                    proof: Some(String::from_utf8(proof).unwrap()),
                    output: None,
                    quote: None,
                }
                .to_response(),
            ))
        }
        // For all other statuses just return the status.
        _ => Ok(status_into_response(*latest_status)),
    }
}

pub fn status_into_response(status: TaskStatus) -> Json<Value> {
    Json(serde_json::json!({
        "status": "ok",
        "data": {
            "status": status,
        }
    }))
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    let mut docs = Docs::openapi();
    docs.merge(cancel::create_docs());
    docs
}

pub fn create_router() -> Router<ProverState> {
    Router::new()
        .route("/", post(proof_handler))
        .nest("/cancel", cancel::create_router())
}
