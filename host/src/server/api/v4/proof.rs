use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::{
        api::{v2, v3::Status},
        utils::to_v3_status,
    },
};
use axum::{extract::State, routing::post, Json, Router};
use raiko_core::interfaces::ProofRequest;
use raiko_reqactor::Actor;
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, BatchProofRequestEntity, BatchProofRequestKey,
    RequestEntity, RequestKey,
};
use serde_json::Value;
use utoipa::OpenApi;

// mod aggregate;
// mod cancel;

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = AggregationRequest,
    responses (
        (status = 200, description = "Successfully submitted proof task, queried tasks in progress or retrieved proof.", body = Status)
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
async fn proof_handler(
    State(actor): State<Actor>,
    Json(batch_request_opt): Json<Value>,
) -> HostResult<Status> {
    inc_current_req();

    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = actor.default_request_config().clone();
    config.merge(&batch_request_opt)?;

    let batch_request: ProofRequest = ProofRequest::try_from(config.clone())?;
    assert!(
        batch_request.block_number == 0,
        "balock_number is 0 in batch mode"
    );
    assert!(
        !batch_request.l1_inclusion_block_number > 0,
        "l1_inclusion_block_number must present in batch mode"
    );

    inc_host_req_count(batch_request.batch_id);
    inc_guest_req_count(&batch_request.proof_type, batch_request.batch_id);

    let chain_id = actor
        .chain_specs()
        .get_chain_spec(&batch_request.network)
        .expect("get a known chainspec")
        .chain_id;

    let request_key = RequestKey::BatchProof(BatchProofRequestKey::new(
        chain_id,
        batch_request.batch_id,
        batch_request.l1_inclusion_block_number,
        batch_request.proof_type,
        batch_request.prover.to_string(),
    ))
    .into();
    let request_entity = RequestEntity::BatchProof(BatchProofRequestEntity::new(
        batch_request.batch_id,
        batch_request.l1_inclusion_block_number,
        batch_request.network,
        batch_request.l1_network,
        batch_request.graffiti,
        batch_request.prover,
        batch_request.proof_type,
        batch_request.blob_proof_type,
        batch_request.prover_args,
    ))
    .into();

    // no need run here as prove_aggregation calls prove() internally
    // let result = crate::server::prove(&actor, request_key, request_entity).await;

    // in batch mode, single batch are aggregated automatically
    let agg_request_key =
        AggregationRequestKey::new(batch_request.proof_type, vec![batch_request.batch_id]);
    let agg_request_entity_without_proofs = AggregationRequestEntity::new(
        vec![batch_request.batch_id],
        vec![],
        batch_request.proof_type,
        config.prover_args,
    );
    let sub_request_keys = vec![request_key];
    let sub_request_entities = vec![request_entity];
    let result = crate::server::prove_aggregation(
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
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        // cancel::create_docs(),
        // aggregate::create_docs(),
        v2::proof::report::create_docs(),
        v2::proof::list::create_docs(),
        v2::proof::prune::create_docs(),
    ]
    .into_iter()
    .fold(Docs::openapi(), |mut docs, curr| {
        docs.merge(curr);
        docs
    })
}

// todo: cancel
pub fn create_router() -> Router<Actor> {
    Router::new()
        .route("/", post(proof_handler))
        // .nest("/cancel", cancel::create_router())
        // .nest("/aggregate", aggregate::create_router())
        .nest("/report", v2::proof::report::create_router())
        .nest("/list", v2::proof::list::create_router())
        .nest("/prune", v2::proof::prune::create_router())
}
