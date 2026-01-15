use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::{
        api::{v2, v3::Status},
        prove_aggregation,
        utils::to_v3_status,
    },
};
use axum::{extract::State, routing::post, Json, Router};
use raiko_core::{
    interfaces::{AggregationRequest, ProofRequest, ProofRequestOpt},
    provider::get_task_data,
};
use raiko_reqactor::Actor;
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, RequestEntity, RequestKey,
    SingleProofRequestEntity, SingleProofRequestKey,
};
use utoipa::OpenApi;

mod aggregate;
mod batch;
mod batch_handler;
mod cancel;
mod shasta_handler;

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
    Json(mut aggregation_request): Json<AggregationRequest>,
) -> HostResult<Status> {
    inc_current_req();

    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    aggregation_request.merge(&actor.default_request_config())?;

    let proof_request_opts: Vec<ProofRequestOpt> = aggregation_request.clone().into();

    if proof_request_opts.is_empty() {
        return Err(anyhow::anyhow!("No blocks for proving provided").into());
    }

    // Construct the actual proof request from the available configs.
    let mut sub_request_keys = Vec::with_capacity(proof_request_opts.len());
    let mut sub_request_entities = Vec::with_capacity(proof_request_opts.len());
    for proof_request_opt in proof_request_opts {
        let proof_request = ProofRequest::try_from(proof_request_opt)?;

        inc_host_req_count(proof_request.block_number);
        inc_guest_req_count(&proof_request.proof_type, proof_request.block_number);

        let (chain_id, blockhash) = get_task_data(
            &proof_request.network,
            proof_request.block_number,
            actor.chain_specs(),
        )
        .await?;

        let request_key = RequestKey::SingleProof(SingleProofRequestKey::new(
            chain_id,
            proof_request.block_number,
            blockhash,
            proof_request.proof_type,
            proof_request.prover.to_string(),
        ));
        let request_entity = RequestEntity::SingleProof(SingleProofRequestEntity::new(
            proof_request.block_number,
            proof_request.l1_inclusion_block_number,
            proof_request.network,
            proof_request.l1_network,
            proof_request.graffiti,
            proof_request.prover,
            proof_request.proof_type,
            proof_request.blob_proof_type,
            proof_request.prover_args,
        ));

        sub_request_keys.push(request_key);
        sub_request_entities.push(request_entity);
    }

    let proof_type = *sub_request_keys.first().unwrap().proof_type();
    let block_numbers = aggregation_request
        .block_numbers
        .iter()
        .map(|(block_number, _)| *block_number)
        .collect::<Vec<_>>();
    let agg_request_key = AggregationRequestKey::new(proof_type, block_numbers.clone());
    let agg_request_entity_without_proofs = AggregationRequestEntity::new(
        block_numbers,
        vec![],
        proof_type,
        aggregation_request.prover_args,
    );

    let result = prove_aggregation(
        &actor,
        agg_request_key.into(),
        agg_request_entity_without_proofs.into(),
        sub_request_keys,
        sub_request_entities,
    )
    .await;
    Ok(to_v3_status(proof_type, None, result))
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        cancel::create_docs(),
        aggregate::create_docs(),
        batch_handler::create_docs(),
        shasta_handler::create_docs(),
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

pub fn create_router() -> Router<Actor> {
    Router::new()
        .route("/", post(proof_handler))
        .nest("/cancel", cancel::create_router())
        .nest("/aggregate", aggregate::create_router())
        .nest("/batch", batch_handler::create_router())
        .nest("/batch/shasta", shasta_handler::create_router())
        .nest("/report", v2::proof::report::create_router())
        .nest("/list", v2::proof::list::create_router())
        .nest("/prune", v2::proof::prune::create_router())
}
