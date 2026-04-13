use crate::{
    interfaces::HostResult,
    server::{
        api::v3::{ProofResponse, Status},
        auth::AuthenticatedApiKey,
        handler::{prove, prove_many, prove_aggregation},
        metrics::{record_shasta_request_in, record_shasta_request_out},
        utils::{draw_shasta_zk_request, is_zk_any_request, to_v3_status},
    },
};
use axum::{extract::State, routing::post, Extension, Json, Router};
use raiko_core::{
    interfaces::{RaikoError, ShastaProofRequest, ShastaProofRequestOpt},
    merge,
};
use raiko_lib::proof_type::ProofType;
use raiko_lib::prover::Proof;
use raiko_lib::utils::shasta_guest_input::{
    encode_guest_input_str_to_prover_arg_value, PROVER_ARG_SHASTA_GUEST_INPUT,
};
use raiko_reqactor::Actor;
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, ImageId, RequestEntity, RequestKey,
    ShastaProofRequestEntity,
};
use raiko_tasks::TaskStatus;
use serde_json::Value;
use utoipa::OpenApi;

use super::batch::process_shasta_batch;

/// Guest-input phase outcome for one Shasta proposal. If the corresponding [`RequestKey::ShastaProof`]
/// is already `Success` in the pool, guest input is **not** required (and may be absent after
/// pool eviction); otherwise we run guest-input `prove` and record its status.
#[derive(Debug)]
enum ShastaGuestInputStep {
    SkippedSubProofDone,
    Ran(raiko_reqpool::Status),
}

/// For each proposal, skip guest-input work when that proposal's `ShastaProof` key is already
/// successful in the pool (depends-on-proof semantics).
async fn run_shasta_guest_inputs_with_subproof_dependency(
    actor: &Actor,
    sub_input_request_keys: Vec<RequestKey>,
    sub_input_request_entities: Vec<RequestEntity>,
    sub_request_keys: &[RequestKey],
) -> Result<Vec<ShastaGuestInputStep>, String> {
    let n = sub_input_request_keys.len();
    if n != sub_request_keys.len() {
        return Err("shasta batch: input keys and proof keys length mismatch".to_string());
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let sub_proof_done = matches!(
            actor.pool_get_status(&sub_request_keys[i]).await?,
            Some(ref ctx) if matches!(ctx.status(), raiko_reqpool::Status::Success { .. })
        );
        if sub_proof_done {
            tracing::info!(
                proposal_index = i,
                "skipping Shasta guest input: corresponding ShastaProof already successful in pool"
            );
            out.push(ShastaGuestInputStep::SkippedSubProofDone);
            continue;
        }
        let st = prove(
            actor,
            sub_input_request_keys[i].clone(),
            sub_input_request_entities[i].clone(),
        )
        .await?;
        out.push(ShastaGuestInputStep::Ran(st));
    }
    Ok(out)
}

fn all_shasta_guest_input_steps_resolved(steps: &[ShastaGuestInputStep]) -> bool {
    steps.iter().all(|step| {
        matches!(
            step,
            ShastaGuestInputStep::SkippedSubProofDone
                | ShastaGuestInputStep::Ran(raiko_reqpool::Status::Success { .. })
        )
    })
}

fn build_shasta_sub_request_entities_with_guest_input(
    sub_request_entities: &[RequestEntity],
    steps: &[ShastaGuestInputStep],
) -> Result<Vec<RequestEntity>, String> {
    if sub_request_entities.len() != steps.len() {
        return Err("shasta batch: proof entities and guest-input steps length mismatch".to_string());
    }
    sub_request_entities
        .iter()
        .zip(steps)
        .map(|(entity, step)| match (entity, step) {
            (_, ShastaGuestInputStep::SkippedSubProofDone) => Ok(entity.clone()),
            (
                RequestEntity::ShastaProof(e),
                ShastaGuestInputStep::Ran(raiko_reqpool::Status::Success { proof }),
            ) => {
                let guest_input = proof.proof.clone().ok_or_else(|| {
                    "guest input success status missing compressed payload".to_string()
                })?;
                let mut prover_args = e.prover_args().clone();
                prover_args.insert(
                    PROVER_ARG_SHASTA_GUEST_INPUT.to_string(),
                    encode_guest_input_str_to_prover_arg_value(&guest_input)?,
                );
                Ok(
                    ShastaProofRequestEntity::new_with_guest_input_entity(
                        e.guest_input_entity().clone(),
                        *e.proof_type(),
                        prover_args,
                    )
                    .into(),
                )
            }
            (RequestEntity::ShastaProof(_), ShastaGuestInputStep::Ran(_)) => Err(
                "guest input step did not succeed for a proposal that still needs proof".to_string(),
            ),
            _ => Err("invalid Shasta proof entity in batch".to_string()),
        })
        .collect()
}

/// When every `ShastaProof` pool entry is already successful, returns those proofs so the handler
/// can skip regenerating guest input (which may have been removed from the pool after proof).
async fn collect_shasta_subproofs_if_all_successful(
    actor: &Actor,
    sub_request_keys: &[RequestKey],
) -> Result<Option<Vec<Proof>>, String> {
    let mut proofs = Vec::with_capacity(sub_request_keys.len());
    for key in sub_request_keys {
        let ctx = match actor.pool_get_status(key).await? {
            Some(c) => c,
            None => return Ok(None),
        };
        match ctx.into_status() {
            raiko_reqpool::Status::Success { proof } => proofs.push(proof),
            _ => return Ok(None),
        }
    }
    Ok(Some(proofs))
}

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

    let result =
        if let Some(existing_proofs) =
            collect_shasta_subproofs_if_all_successful(&actor, &sub_request_keys).await?
        {
            tracing::info!(
                n = existing_proofs.len(),
                aggregate = shasta_request.aggregate,
                "Shasta batch: all sub-proofs already in pool; skipping guest input step"
            );
            if shasta_request.aggregate {
                prove(
                    &actor,
                    RequestKey::ShastaAggregation(AggregationRequestKey::new_with_image_id_and_prover(
                        shasta_request.proof_type,
                        sub_batch_ids.clone(),
                        image_id.clone(),
                        shasta_request.prover.to_string(),
                    )),
                    RequestEntity::ShastaAggregation(AggregationRequestEntity::new(
                        sub_batch_ids,
                        existing_proofs,
                        shasta_request.proof_type,
                        shasta_request.prover_args.clone(),
                    )),
                )
                .await
            } else {
                Ok(existing_proofs
                    .into_iter()
                    .next()
                    .map(|proof| raiko_reqpool::Status::Success { proof })
                    .unwrap_or_else(|| raiko_reqpool::Status::Failed {
                        error: "empty shasta batch".to_string(),
                    }))
            }
        } else {
            // Guest input only where the corresponding ShastaProof is not already successful
            // (proof may exist while guest-input row was evicted — do not redo input in that case).
            let guest_input_steps = run_shasta_guest_inputs_with_subproof_dependency(
                &actor,
                sub_input_request_keys,
                sub_input_request_entities,
                &sub_request_keys,
            )
            .await?;

            if !all_shasta_guest_input_steps_resolved(&guest_input_steps) {
                Ok(raiko_reqpool::Status::Registered)
            } else {
                let sub_request_entities_with_input = build_shasta_sub_request_entities_with_guest_input(
                    &sub_request_entities,
                    &guest_input_steps,
                )?;

                if shasta_request.aggregate {
                    prove_aggregation(
                        &actor,
                        RequestKey::ShastaAggregation(
                            AggregationRequestKey::new_with_image_id_and_prover(
                                shasta_request.proof_type,
                                sub_batch_ids.clone(),
                                image_id.clone(),
                                shasta_request.prover.to_string(),
                            ),
                        ),
                        RequestEntity::ShastaAggregation(AggregationRequestEntity::new(
                            sub_batch_ids,
                            vec![],
                            shasta_request.proof_type,
                            shasta_request.prover_args.clone(),
                        )),
                        sub_request_keys,
                        sub_request_entities_with_input,
                    )
                    .await
                } else {
                    prove_many(&actor, sub_request_keys, sub_request_entities_with_input)
                        .await
                        .map(|s| {
                            s.into_iter()
                                .next()
                                .unwrap_or_else(|| raiko_reqpool::Status::Failed {
                                    error: "No status returned".to_string(),
                                })
                        })
                }
            }
        };

    let status = to_v3_status(shasta_request.proof_type, None, result);
    record_shasta_request_out(&authenticated_key.name, &shasta_request, false);

    Ok(status)
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
