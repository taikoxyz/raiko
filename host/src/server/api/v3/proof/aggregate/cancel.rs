use crate::{
    interfaces::{HostError, HostResult},
    server::{api::v2::CancelStatus, utils::to_v2_cancel_status},
};
use axum::{extract::State, routing::post, Json, Router};
use raiko_core::interfaces::AggregationOnlyRequest;
use raiko_lib::proof_type::ProofType;
use raiko_reqactor::Actor;
use raiko_reqpool::AggregationRequestKey;
use std::str::FromStr;
use utoipa::OpenApi;

#[utoipa::path(post, path = "/proof/aggregate/cancel",
    tag = "Proving",
    request_body = AggregationOnlyRequest,
    responses (
        (status = 200, description = "Successfully cancelled proof aggregation task", body = CancelStatus)
    )
)]
/// Cancel a proof aggregation task with requested config.
///
/// Accepts a proof aggregation request and cancels a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn cancel_handler(
    State(actor): State<Actor>,
    Json(mut aggregation_request): Json<AggregationOnlyRequest>,
) -> HostResult<CancelStatus> {
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    aggregation_request.merge(&actor.default_request_config())?;

    let proof_type = ProofType::from_str(
        aggregation_request
            .proof_type
            .as_deref()
            .unwrap_or_default(),
    )
    .map_err(HostError::Conversion)?;
    let agg_request_key =
        AggregationRequestKey::new(proof_type, aggregation_request.aggregation_ids).into();
    let result = crate::server::cancel(&actor, agg_request_key).await;
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
