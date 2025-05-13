use axum::{extract::State, routing::post, Json, Router};
use raiko_core::{interfaces::ProofRequest, provider::get_task_data};
use raiko_reqactor::Actor;
use raiko_reqpool::{SingleProofRequestEntity, SingleProofRequestKey};
use serde_json::Value;
use tracing::warn;
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::{
        api::v1::Status,
        utils::{draw_for_zk_any_request, fulfill_sp1_params, is_zk_any_request, to_v1_status},
    },
};

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully created proof for request", body = Status)
    )
)]
/// Generate a proof for requested config.
///
/// Accepts a proof request and generates a proof with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn proof_handler(
    State(actor): State<Actor>,
    Json(mut req): Json<Value>,
) -> HostResult<Json<Status>> {
    warn!("Using deprecated v1 proof handler");
    inc_current_req();

    if is_zk_any_request(&req) {
        fulfill_sp1_params(&mut req);
    }

    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = actor.default_request_config().clone();
    config.merge(&req)?;

    // For zk_any request, draw zk proof type based on the block hash.
    if is_zk_any_request(&req) {
        match draw_for_zk_any_request(&actor, &serde_json::to_value(&config)?).await? {
            Some(proof_type) => config.proof_type = Some(proof_type.to_string()),
            None => {
                return Err(
                    anyhow::anyhow!("Failed to draw zk_any proof type for block hash").into(),
                );
            }
        }
    }

    if let Some(ref proof_type) = config.proof_type {
        match proof_type.as_str() {
            "risc0" => {
                #[cfg(not(feature = "risc0"))]
                return Err(anyhow::anyhow!("RISC0 not supported").into());
            }
            "sp1" => {
                #[cfg(not(feature = "sp1"))]
                return Err(anyhow::anyhow!("SP1 not supported").into());
            }
            "sgx" => {
                #[cfg(not(feature = "sgx"))]
                return Err(anyhow::anyhow!("SGX not supported").into());
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown proof type: {}", proof_type).into());
            }
        }
    }

    // Construct the actual proof request from the available configs.
    let proof_request = ProofRequest::try_from(config)?;
    inc_host_req_count(proof_request.block_number);
    inc_guest_req_count(&proof_request.proof_type, proof_request.block_number);

    let (chain_id, blockhash) = get_task_data(
        &proof_request.network,
        proof_request.block_number,
        actor.chain_specs(),
    )
    .await?;

    let request_key = SingleProofRequestKey::new(
        chain_id,
        proof_request.block_number,
        blockhash,
        proof_request.proof_type,
        proof_request.prover.to_string(),
    )
    .into();
    let request_entity = SingleProofRequestEntity::new(
        proof_request.block_number,
        proof_request.l1_inclusion_block_number,
        proof_request.network,
        proof_request.l1_network,
        proof_request.graffiti,
        proof_request.prover,
        proof_request.proof_type,
        proof_request.blob_proof_type,
        proof_request.prover_args,
    )
    .into();

    let result = crate::server::wait_prove(&actor, request_key, request_entity).await?;

    Ok(to_v1_status(result))
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", post(proof_handler))
}
