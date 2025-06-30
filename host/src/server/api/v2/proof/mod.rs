use axum::{extract::State, routing::post, Json, Router};
use raiko_core::{interfaces::ProofRequest, provider::get_task_data};
use raiko_lib::proof_type::ProofType;
use raiko_reqpool::{
    GuestInputRequestEntity, GuestInputRequestKey, SingleProofRequestEntity, SingleProofRequestKey,
    Status as InPoolStatus,
};
use raiko_tasks::TaskStatus;
use serde_json::Value;
use utoipa::OpenApi;

use crate::server::utils::{draw_for_zk_any_request, is_zk_any_request};
use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::{api::v2::Status, to_v2_status},
};
use raiko_reqactor::Actor;

use super::ProofResponse;

pub mod cancel;
pub mod list;
pub mod prune;
pub mod report;

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully submitted proof task, queried tasks in progress or retrieved proof.", body = Status)
    )
)]
/// Submit a proof task with requested config, get task status or get proof value.
///
/// Accepts a proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn proof_handler(State(actor): State<Actor>, Json(req): Json<Value>) -> HostResult<Status> {
    inc_current_req();

    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = actor.default_request_config().clone();
    config.merge(&req)?;

    // For zk_any request, draw zk proof type based on the block hash.
    if is_zk_any_request(&req) {
        match draw_for_zk_any_request(&actor, &serde_json::to_value(&config)?).await? {
            Some(proof_type) => config.proof_type = Some(proof_type.to_string()),
            None => {
                return Ok(Status::Ok {
                    proof_type: ProofType::Native,
                    batch_id: None,
                    data: ProofResponse::Status {
                        status: TaskStatus::ZKAnyNotDrawn,
                    },
                });
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

    let proof_type = proof_request.proof_type;
    let request_key =
        GuestInputRequestKey::new(chain_id, proof_request.block_number, blockhash).into();
    let request_entity = GuestInputRequestEntity::new(
        proof_request.block_number,
        proof_request.l1_inclusion_block_number,
        proof_request.network.clone(),
        proof_request.l1_network.clone(),
        proof_request.graffiti,
        proof_request.blob_proof_type.clone(),
        proof_request.prover_args.clone(),
    )
    .into();

    // deal with guest input first
    let result = crate::server::prove(&actor, request_key, request_entity).await;
    match result {
        Ok(InPoolStatus::Success { proof }) => {
            let request_key = SingleProofRequestKey::new(
                chain_id,
                proof_request.block_number,
                blockhash,
                proof_request.proof_type,
                proof_request.prover.to_string(),
            )
            .into();
            let mut prover_args = proof_request.prover_args.clone();
            prover_args.insert(
                "guest_input".to_string(),
                serde_json::to_value(proof.proof)?,
            );
            let request_entity = SingleProofRequestEntity::new(
                proof_request.block_number,
                proof_request.l1_inclusion_block_number,
                proof_request.network,
                proof_request.l1_network,
                proof_request.graffiti,
                proof_request.prover,
                proof_request.proof_type,
                proof_request.blob_proof_type,
                prover_args,
            )
            .into();

            let result = crate::server::prove(&actor, request_key, request_entity).await;
            Ok(to_v2_status(proof_type, None, result))
        }
        Ok(_) => Ok(to_v2_status(proof_type, None, result)),
        Err(_) => Ok(to_v2_status(proof_type, None, result)),
    }
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        cancel::create_docs(),
        report::create_docs(),
        list::create_docs(),
        prune::create_docs(),
    ]
    .into_iter()
    .fold(Docs::openapi(), |mut docs, curr| {
        docs.merge(curr);
        docs
    })
}

pub fn create_router() -> Router<Actor> {
    Router::new()
        .route("/", post(proof_handler))
        .nest("/cancel", cancel::create_router())
        .nest("/report", report::create_router())
        .nest("/list", list::create_router())
        .nest("/prune", prune::create_router())
}
