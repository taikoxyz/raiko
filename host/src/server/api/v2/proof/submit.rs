use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::interfaces::ProofRequest;
use serde_json::Value;
use tracing::info;
use utoipa::OpenApi;

use crate::{
    interfaces::{HostError, HostResult},
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    ProverState,
};

#[utoipa::path(post, path = "/proof/submit",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully submitted proof task", body = Status)
    )
)]
#[debug_handler(state = ProverState)]
/// Submit a proof task with requested config.
///
/// Accepts a proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn submit_handler(
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

    info!(
        "# Generating proof for block {} on {}",
        proof_request.block_number, proof_request.network
    );
    prover_state
        .task_channel
        .try_send((proof_request, prover_state.opts))
        .map_err(|e| match e {
            tokio::sync::mpsc::error::TrySendError::Full(_) => HostError::CapacityFull,
            tokio::sync::mpsc::error::TrySendError::Closed(_) => HostError::HandleDropped,
        })?;
    let task_db = prover_state.task_db.lock().await;
    let mut manager = task_db.manage()?;
    #[allow(unreachable_code)]
    manager.enqueue_task(
        // TODO:(petar) implement task details here
        todo!(),
    )?;
    Ok(Json(serde_json::json!("{}")))
    // handle_proof(prover_state, req).await.map_err(|e| {
    //     dec_current_req();
    //     e
    // })
}

#[derive(OpenApi)]
#[openapi(paths(submit_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/submit", post(submit_handler))
}
