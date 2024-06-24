use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::interfaces::ProofRequest;
use raiko_task_manager::TaskDb;
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

    let l1_chain_spec = prover_state
        .chain_specs
        .get_chain_spec(&proof_request.l1_network.to_string())
        .ok_or_else(|| HostError::InvalidRequestConfig("Unsupported l1 network".to_string()))?;

    let db = TaskDb::open_or_create(&prover_state.opts.sqlite_file)?;
    // db.set_tracer(Some(|stmt| println!("sqlite:\n-------\n{}\n=======", stmt)));
    let mut manager = db.manage()?;

    prover_state.task_channel.try_send((
        proof_request.clone(),
        prover_state.opts,
        prover_state.chain_specs,
    ))?;

    manager.enqueue_task(l1_chain_spec.chain_id, &proof_request)?;
    Ok(Json(serde_json::json!("{}")))
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
