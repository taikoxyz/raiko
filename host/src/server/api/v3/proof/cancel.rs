use axum::{extract::State, routing::post, Json, Router};
use raiko_core::{
    interfaces::{AggregationRequest, ProofRequest, ProofRequestOpt},
    provider::get_task_data,
};
use raiko_reqpool::{AggregationRequestKey, SingleProofRequestKey};
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    server::{api::v2::CancelStatus, to_v2_cancel_status},
};
use raiko_reqactor::Actor;

#[utoipa::path(post, path = "/proof/cancel",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully cancelled proof task", body = CancelStatus)
    )
)]
/// Cancel a proof task with requested config.
///
/// Accepts a proof request and cancels a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
pub async fn cancel_handler(
    State(actor): State<Actor>,
    Json(mut aggregation_request): Json<AggregationRequest>,
) -> HostResult<CancelStatus> {
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let default_request_config = actor.default_request_config();
    aggregation_request.merge(default_request_config)?;

    let proof_request_opts: Vec<ProofRequestOpt> = aggregation_request.into();

    let mut sub_request_keys = Vec::with_capacity(proof_request_opts.len());
    for opt in proof_request_opts {
        let proof_request = ProofRequest::try_from(opt)?;

        let (chain_id, block_hash) = get_task_data(
            &proof_request.network,
            proof_request.block_number,
            actor.chain_specs(),
        )
        .await?;

        let sub_key = SingleProofRequestKey::new(
            chain_id,
            proof_request.block_number,
            block_hash,
            proof_request.proof_type,
            proof_request.prover.clone().to_string(),
        );
        sub_request_keys.push(sub_key);
    }

    let proof_type = sub_request_keys.first().unwrap().proof_type();
    let block_numbers = sub_request_keys
        .iter()
        .map(|key| *key.block_number())
        .collect();
    let agg_request_key = AggregationRequestKey::new(*proof_type, block_numbers);

    let result = crate::server::cancel_aggregation(&actor, agg_request_key, sub_request_keys).await;
    Ok(to_v2_cancel_status(result))
}

#[derive(OpenApi)]
#[openapi(paths(cancel_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", post(cancel_handler))
}
