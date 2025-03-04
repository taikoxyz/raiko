use crate::{
    interfaces::HostResult,
    server::{api::v3::Status, prove_aggregation, utils::to_v3_status},
};
use axum::{extract::State, routing::post, Json, Router};
use raiko_core::{
    interfaces::{BatchMetadata, BatchProofRequest},
    merge,
};
use raiko_reqactor::Actor;
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, BatchProofRequestEntity, BatchProofRequestKey,
};
use serde_json::Value;
use utoipa::OpenApi;

#[utoipa::path(post, path = "/batch",
    tag = "Proving",
    request_body = BatchProofRequest,
    responses (
        (status = 200, description = "Successfully submitted batch proof task, queried batch tasks in progress or retrieved batch proof.", body = Status)
    )
)]
/// Submit a batch proof task with requested config, get task status or get proof value.
///
/// Accepts a batch proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn batch_handler(
    State(actor): State<Actor>,
    Json(batch_request_opt): Json<Value>,
) -> HostResult<Status> {
    let batch_request = {
        // Override the existing proof request config from the config file and command line
        // options with the request from the client, and convert to a BatchProofRequest.
        let mut opts = serde_json::to_value(actor.default_request_config())?;
        merge(&mut opts, &batch_request_opt);
        let batch_request: BatchProofRequest = serde_json::from_value(opts)?;

        // Validate the batch request
        if batch_request.batches.is_empty() {
            return Err(anyhow::anyhow!("batches is empty").into());
        }

        batch_request
    };
    tracing::info!(
        "IN Batch request: {}",
        serde_json::to_string(&batch_request)?
    );

    let chain_id = actor.get_chain_spec(&batch_request.network)?.chain_id;
    let mut sub_request_keys = Vec::with_capacity(batch_request.batches.len());
    let mut sub_request_entities = Vec::with_capacity(batch_request.batches.len());
    let mut sub_batch_ids = Vec::with_capacity(batch_request.batches.len());
    for BatchMetadata {
        batch_id,
        l1_inclusion_block_number,
    } in batch_request.batches.iter()
    {
        let request_key = BatchProofRequestKey::new(
            chain_id,
            *batch_id,
            *l1_inclusion_block_number,
            batch_request.proof_type,
            batch_request.prover.to_string(),
        )
        .into();
        let request_entity = BatchProofRequestEntity::new(
            *batch_id,
            *l1_inclusion_block_number,
            batch_request.network.clone(),
            batch_request.l1_network.clone(),
            batch_request.graffiti.clone(),
            batch_request.prover.clone(),
            batch_request.proof_type,
            batch_request.blob_proof_type.clone(),
            batch_request.prover_args.clone().into(),
        )
        .into();
        sub_request_keys.push(request_key);
        sub_request_entities.push(request_entity);
        sub_batch_ids.push(*batch_id);
    }

    let agg_request_key =
        AggregationRequestKey::new(batch_request.proof_type, sub_batch_ids.clone()).into();
    let agg_request_entity_without_proofs = AggregationRequestEntity::new(
        sub_batch_ids,
        vec![],
        batch_request.proof_type,
        batch_request.prover_args,
    )
    .into();
    let result = prove_aggregation(
        &actor,
        agg_request_key,
        agg_request_entity_without_proofs,
        sub_request_keys,
        sub_request_entities,
    )
    .await;

    Ok(to_v3_status(batch_request.proof_type, result))
}

#[derive(OpenApi)]
#[openapi(paths(batch_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", post(batch_handler))
}
