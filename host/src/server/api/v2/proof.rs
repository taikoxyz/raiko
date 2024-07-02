use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::interfaces::ProofRequest;
use raiko_core::provider::get_task_data;
use raiko_task_manager::{get_task_manager, EnqueueTaskParams, TaskManager, TaskStatus};
use serde_json::Value;
use tracing::info;
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::api::v1::ProofResponse,
    ProverState,
};

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

    if status.is_empty() {
        info!(
            "# Generating proof for block {} on {}",
            proof_request.block_number, proof_request.network
        );

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

        return Ok(Json(serde_json::json!(
            {
                "status": "ok",
                "data": {
                    "status": TaskStatus::Registered,
                }
            }
        )));
    }

    let status = status.last().unwrap().0;

    if matches!(status, TaskStatus::Success) {
        let proof = manager
            .get_task_proof(
                chain_id,
                block_hash,
                proof_request.proof_type,
                Some(proof_request.prover.to_string()),
            )
            .await?;

        let response = ProofResponse {
            proof: Some(String::from_utf8(proof).unwrap()),
            output: None,
            quote: None,
        };

        return Ok(Json(response.to_response()));
    }

    Ok(Json(serde_json::json!(
        {
            "status": "ok",
            "data": {
                "status": status,
            }
        }
    )))
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", post(proof_handler))
}
