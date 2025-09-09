use crate::{
    interfaces::HostResult,
    server::{
        api::v3::Status,
        auth::AuthenticatedApiKey,
        handler::prove_many,
        metrics::{record_shasta_request_in, record_shasta_request_out},
        prove_aggregation,
        utils::to_v3_status,
    },
};
use axum::{extract::State, routing::post, Extension, Json, Router};
use raiko_core::interfaces::ShastaProofRequest;
use raiko_reqactor::Actor;
use raiko_reqpool::{AggregationRequestEntity, AggregationRequestKey, ImageId};
use serde_json::Value;
use utoipa::OpenApi;

use super::batch::process_shasta_batch;

#[utoipa::path(post, path = "/batch/shasta",
    tag = "Proving",
    request_body = ShastaProofRequest,
    responses (
        (status = 200, description = "Successfully submitted Shasta batch proof task, queried batch tasks in progress or retrieved batch proof.", body = Status)
    )
)]
/// Submit a Shasta batch proof task with requested config, get task status or get proof value.
///
/// Accepts a Shasta batch proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn shasta_batch_handler(
    State(actor): State<Actor>,
    Extension(authenticated_key): Extension<AuthenticatedApiKey>,
    Json(shasta_request_opt): Json<Value>,
) -> HostResult<Status> {
    tracing::debug!(
        "Incoming Shasta batch request: {} from {}",
        serde_json::to_string(&shasta_request_opt)?,
        authenticated_key.name
    );

    let shasta_request: ShastaProofRequest = serde_json::from_value(shasta_request_opt)?;
    record_shasta_request_in(&authenticated_key.name, &shasta_request);
    tracing::info!(
        "Accepted {}'s Shasta batch request: {}",
        authenticated_key.name,
        serde_json::to_string(&shasta_request)?,
    );

    // Create image ID based on proof type for provers
    let image_id = ImageId::from_proof_type_and_request_type(
        &shasta_request.proof_type,
        shasta_request.aggregate,
    );

    let (
        _sub_input_request_keys,
        sub_request_keys,
        _sub_input_request_entities,
        sub_request_entities,
        sub_batch_ids,
    ) = process_shasta_batch(&shasta_request, &image_id);

    let result = if shasta_request.aggregate {
        prove_aggregation(
            &actor,
            AggregationRequestKey::new_with_image_id(
                shasta_request.proof_type,
                sub_batch_ids.clone(),
                image_id.clone(),
            ),
            AggregationRequestEntity::new(
                sub_batch_ids,
                vec![],
                shasta_request.proof_type,
                shasta_request.prover_args.clone(),
            ),
            sub_request_keys,
            sub_request_entities,
        )
        .await
    } else {
        prove_many(&actor, sub_request_keys, sub_request_entities)
            .await
            .map(|statuses| {
                statuses
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| raiko_reqpool::Status::Failed {
                        error: "No status returned".to_string(),
                    })
            })
    };

    let status = to_v3_status(shasta_request.proof_type, None, result);
    record_shasta_request_out(&authenticated_key.name, &shasta_request, false);

    Ok(status)
}

#[derive(OpenApi)]
#[openapi(paths(shasta_batch_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", post(shasta_batch_handler))
}
