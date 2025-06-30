use std::str::FromStr;

use axum::{extract::State, routing::post, Json, Router};
use raiko_core::interfaces::AggregationOnlyRequest;
use raiko_lib::proof_type::ProofType;
use raiko_reqpool::{AggregationRequestEntity, AggregationRequestKey};
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::{api::v3::Status, to_v3_status, HostError},
};
use raiko_reqactor::Actor;

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
/// Submit a proof aggregation task with requested config, get task status or get proof value.
///
/// Accepts a proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn aggregation_handler(
    State(actor): State<Actor>,
    Json(mut aggregation_request): Json<AggregationOnlyRequest>,
) -> HostResult<Status> {
    inc_current_req();
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let default_request_config = actor.default_request_config();
    aggregation_request.merge(&default_request_config)?;

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

    let agg_request_key =
        AggregationRequestKey::new(proof_type, aggregation_request.aggregation_ids.clone()).into();
    let agg_request_entity = AggregationRequestEntity::new(
        aggregation_request.aggregation_ids,
        aggregation_request.proofs,
        proof_type,
        aggregation_request.prover_args,
    )
    .into();

    let result = crate::server::prove(&actor, agg_request_key, agg_request_entity).await;
    Ok(to_v3_status(proof_type, None, result))
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

pub fn create_router() -> Router<Actor> {
    Router::new()
        .route("/", post(aggregation_handler))
        .nest("/cancel", cancel::create_router())
        .nest("/prune", prune::create_router())
        .nest("/report", report::create_router())
}
