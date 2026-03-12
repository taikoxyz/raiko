use crate::{
    interfaces::HostResult,
    server::{
        api::v3::{ProofResponse, Status},
        auth::AuthenticatedApiKey,
        handler::prove_many,
        metrics::{record_shasta_request_in, record_shasta_request_out},
        prove_aggregation,
        utils::{draw_shasta_zk_request, is_zk_any_request, to_v3_status},
    },
};
use axum::{extract::State, routing::post, Extension, Json, Router};
use raiko_core::{
    interfaces::{RaikoError, ShastaProofRequest, ShastaProofRequestOpt},
    merge,
};
use raiko_lib::proof_type::ProofType;
use raiko_lib::utils::shasta_guest_input::{
    encode_guest_input_str_to_prover_arg_value, PROVER_ARG_SHASTA_GUEST_INPUT,
};
use raiko_reqactor::Actor;
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, ImageId, RequestEntity, RequestKey,
    ShastaProofRequestEntity, Status as ReqpoolStatus,
};
use raiko_tasks::TaskStatus;
use serde_json::Value;
use utoipa::OpenApi;

use super::batch::process_shasta_batch;

/// Resolves proof_type for zk_any requests. Returns Some(Status) to return early, or None to continue.
async fn resolve_zk_any_proof_type(
    actor: &Actor,
    shasta_request_opt: &mut Value,
) -> HostResult<Option<Status>> {
    if !is_zk_any_request(shasta_request_opt) {
        return Ok(None);
    }

    let proposals =
        shasta_request_opt["proposals"]
            .as_array()
            .ok_or(RaikoError::InvalidRequestConfig(
                "Missing proposals".to_string(),
            ))?;
    let first_batch = proposals.first().ok_or(RaikoError::InvalidRequestConfig(
        "batches is empty".to_string(),
    ))?;
    let first_batch_id = first_batch["proposal_id"]
        .as_u64()
        .ok_or_else(|| RaikoError::InvalidRequestConfig("Missing proposal_id".to_string()))?;
    let l1_inclusion_block = first_batch["l1_inclusion_block_number"]
        .as_u64()
        .ok_or_else(|| {
            RaikoError::InvalidRequestConfig("Missing l1_inclusion_block_number".to_string())
        })?;

    match draw_shasta_zk_request(actor, first_batch_id, l1_inclusion_block).await? {
        Some(proof_type) => {
            shasta_request_opt["proof_type"] = serde_json::to_value(proof_type).unwrap();
            Ok(None)
        }
        None => Ok(Some(Status::Ok {
            proof_type: ProofType::Native,
            batch_id: Some(first_batch_id),
            data: ProofResponse::Status {
                status: TaskStatus::ZKAnyNotDrawn,
            },
        })),
    }
}

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
    Json(mut shasta_request_opt): Json<Value>,
) -> HostResult<Status> {
    tracing::info!(
        "Incoming Shasta batch request: {} from {}",
        serde_json::to_string(&shasta_request_opt)?,
        authenticated_key.name
    );

    // For zk_any request, draw proof type from block hash; return early if not drawn.
    if let Some(early_status) = resolve_zk_any_proof_type(&actor, &mut shasta_request_opt).await? {
        return Ok(early_status);
    }

    let shasta_request: ShastaProofRequest = finalize_shasta_request(&actor, shasta_request_opt)?;

    record_shasta_request_in(&authenticated_key.name, &shasta_request);
    tracing::info!(
        "Accepted {}'s Shasta proposal request: {}",
        authenticated_key.name,
        serde_json::to_string(&shasta_request)?,
    );

    // Create image ID based on proof type for provers
    let image_id = ImageId::from_proof_type_and_request_type(
        &shasta_request.proof_type,
        shasta_request.aggregate,
    );

    let (
        sub_input_request_keys,
        sub_request_keys,
        sub_input_request_entities,
        sub_request_entities,
        sub_batch_ids,
    ) = process_shasta_batch(&shasta_request, &image_id);

    let result = if shasta_request.aggregate {
        prove_aggregation(
            &actor,
            RequestKey::ShastaAggregation(AggregationRequestKey::new_with_image_id(
                shasta_request.proof_type,
                sub_batch_ids.clone(),
                image_id.clone(),
            )),
            RequestEntity::ShastaAggregation(AggregationRequestEntity::new(
                sub_batch_ids,
                vec![],
                shasta_request.proof_type,
                shasta_request.prover_args.clone(),
            )),
            sub_request_keys,
            sub_request_entities,
        )
        .await
    } else {
        prove_shasta_batch_non_aggregation(
            &actor,
            sub_input_request_keys,
            sub_input_request_entities,
            sub_request_keys,
            &sub_request_entities,
        )
        .await
    };

    let status = to_v3_status(shasta_request.proof_type, None, result);
    record_shasta_request_out(&authenticated_key.name, &shasta_request, false);

    Ok(status)
}

/// Runs the two-phase Shasta proof flow: guest input first, then proof with encoded guest input.
async fn prove_shasta_batch_non_aggregation(
    actor: &Actor,
    sub_input_request_keys: Vec<RequestKey>,
    sub_input_request_entities: Vec<RequestEntity>,
    sub_request_keys: Vec<RequestKey>,
    sub_request_entities: &[RequestEntity],
) -> Result<ReqpoolStatus, String> {
    let statuses = prove_many(actor, sub_input_request_keys, sub_input_request_entities).await?;
    let all_success = statuses
        .iter()
        .all(|s| matches!(s, ReqpoolStatus::Success { .. }));

    if !all_success {
        return Ok(ReqpoolStatus::Registered);
    }

    let guest_inputs: Vec<_> = statuses
        .iter()
        .map(|s| match s {
            ReqpoolStatus::Success { proof, .. } => proof.proof.clone().unwrap(),
            _ => unreachable!("all_success checked above"),
        })
        .collect();

    let proof_entities: Vec<RequestEntity> = sub_request_entities
        .iter()
        .zip(guest_inputs)
        .map(|(entity, guest_input)| {
            let request_entity = match entity {
                RequestEntity::ShastaProof(e) => e,
                _ => unreachable!("Shasta batch only produces ShastaProof entities"),
            };
            let mut prover_args = request_entity.prover_args().clone();
            prover_args.insert(
                PROVER_ARG_SHASTA_GUEST_INPUT.to_string(),
                encode_guest_input_str_to_prover_arg_value(&guest_input)
                    .expect("failed to wrap shasta_guest_input string"),
            );
            ShastaProofRequestEntity::new_with_guest_input_entity(
                request_entity.guest_input_entity().clone(),
                *request_entity.proof_type(),
                prover_args,
            )
            .into()
        })
        .collect();

    let proof_statuses = prove_many(actor, sub_request_keys, proof_entities).await?;
    Ok(proof_statuses
        .into_iter()
        .next()
        .unwrap_or_else(|| ReqpoolStatus::Failed {
            error: "No status returned".to_string(),
        }))
}

fn finalize_shasta_request(
    actor: &Actor,
    shasta_request_opt: Value,
) -> Result<ShastaProofRequest, RaikoError> {
    let mut opts = serde_json::to_value(actor.default_request_config())?;
    // Override the existing proof request config from the config file and command line
    // options with the request from the client, and convert to ShastaProofRequest.
    merge(&mut opts, &shasta_request_opt);

    let shasta_request_opt: ShastaProofRequestOpt = serde_json::from_value(opts)?;
    let shasta_request: ShastaProofRequest = shasta_request_opt.try_into()?;

    // Validate the batch request
    if shasta_request.proposals.is_empty() {
        return Err(anyhow::anyhow!("proposals is empty").into());
    }

    Ok(shasta_request)
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
