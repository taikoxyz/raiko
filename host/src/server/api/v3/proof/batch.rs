use crate::{
    interfaces::HostResult,
    server::{
        api::v3::{ProofResponse, Status},
        handler::prove_many,
        prove_aggregation,
        utils::{
            draw_for_zk_any_batch_request, fulfill_sp1_params, is_zk_any_request, to_v3_status,
        },
    },
};
use axum::{extract::State, routing::post, Json, Router};
use raiko_core::{
    interfaces::{BatchMetadata, BatchProofRequest, BatchProofRequestOpt},
    merge,
};
use raiko_lib::{proof_type::ProofType, prover::Proof};
use raiko_reqactor::Actor;
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, BatchGuestInputRequestEntity,
    BatchGuestInputRequestKey, BatchProofRequestEntity, BatchProofRequestKey,
};
use raiko_tasks::TaskStatus;
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
    Json(mut batch_request_opt): Json<Value>,
) -> HostResult<Status> {
    tracing::debug!(
        "Received batch request: {}",
        serde_json::to_string(&batch_request_opt)?
    );

    if is_zk_any_request(&batch_request_opt) {
        fulfill_sp1_params(&mut batch_request_opt);
    }

    let batch_request = {
        // Override the existing proof request config from the config file and command line
        // options with the request from the client, and convert to a BatchProofRequest.
        let mut opts = serde_json::to_value(actor.default_request_config())?;
        merge(&mut opts, &batch_request_opt);

        // For zk_any request, draw zk proof type based on the block hash.
        if is_zk_any_request(&opts) {
            match draw_for_zk_any_batch_request(&actor, &opts).await? {
                Some(proof_type) => opts["proof_type"] = serde_json::to_value(proof_type).unwrap(),
                None => {
                    return Ok(Status::Ok {
                        proof_type: ProofType::Native,
                        data: ProofResponse::Status {
                            status: TaskStatus::ZKAnyNotDrawn,
                        },
                    });
                }
            }
        }

        let batch_request_opt: BatchProofRequestOpt = serde_json::from_value(opts)?;
        let batch_request: BatchProofRequest = batch_request_opt.try_into()?;

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
    let mut sub_input_request_keys = Vec::with_capacity(batch_request.batches.len());
    let mut sub_input_request_entities = Vec::with_capacity(batch_request.batches.len());
    let mut sub_request_keys = Vec::with_capacity(batch_request.batches.len());
    let mut sub_request_entities = Vec::with_capacity(batch_request.batches.len());
    let mut sub_batch_ids = Vec::with_capacity(batch_request.batches.len());
    for BatchMetadata {
        batch_id,
        l1_inclusion_block_number,
    } in batch_request.batches.iter()
    {
        let input_request_key =
            BatchGuestInputRequestKey::new(chain_id, *batch_id, *l1_inclusion_block_number);
        let request_key = BatchProofRequestKey::new_with_input_key(
            input_request_key.clone(),
            batch_request.proof_type,
            batch_request.prover.to_string(),
        );

        let input_request_entity = BatchGuestInputRequestEntity::new(
            *batch_id,
            *l1_inclusion_block_number,
            batch_request.network.clone(),
            batch_request.l1_network.clone(),
            batch_request.graffiti.clone(),
            batch_request.blob_proof_type.clone(),
        );
        let request_entity = BatchProofRequestEntity::new_with_guest_input_entity(
            input_request_entity.clone(),
            batch_request.prover.clone(),
            batch_request.proof_type,
            batch_request.prover_args.clone().into(),
        );

        sub_input_request_keys.push(input_request_key.into());
        sub_request_keys.push(request_key.into());
        sub_input_request_entities.push(input_request_entity.into());
        sub_request_entities.push(request_entity.into());
        sub_batch_ids.push(*batch_id);
    }

    let result = if batch_request.aggregate {
        prove_aggregation(
            &actor,
            AggregationRequestKey::new(batch_request.proof_type, sub_batch_ids.clone()).into(),
            AggregationRequestEntity::new(
                sub_batch_ids,
                vec![],
                batch_request.proof_type,
                batch_request.prover_args,
            )
            .into(),
            sub_request_keys,
            sub_request_entities,
        )
        .await
    } else {
        let statuses =
            prove_many(&actor, sub_input_request_keys, sub_input_request_entities).await?;
        let is_all_sub_success = statuses
            .iter()
            .all(|status| matches!(status, raiko_reqpool::Status::Success { .. }));
        if !is_all_sub_success {
            Ok(raiko_reqpool::Status::Registered)
        } else {
            let guest_inputs_of_entities = statuses
                .iter()
                .map(|status| match status {
                    raiko_reqpool::Status::Success { proof, .. } => proof.proof.clone().unwrap(),
                    _ => unreachable!("is_all_sub_success checked"),
                })
                .collect::<Vec<_>>();
            let sub_request_entities = sub_request_entities
                .iter()
                .zip(guest_inputs_of_entities)
                .to_owned()
                .map(|(entity, guest_input)| match entity {
                    raiko_reqpool::RequestEntity::BatchProof(request_entity) => {
                        let mut prover_args = request_entity.prover_args().clone();
                        prover_args.insert(
                            "batch_guest_input".to_string(),
                            serde_json::to_value(guest_input).expect(""),
                        );
                        BatchProofRequestEntity::new_with_guest_input_entity(
                            request_entity.guest_input_entity().clone(),
                            request_entity.prover().clone(),
                            *request_entity.proof_type(),
                            prover_args,
                        )
                        .into()
                    }
                    _ => entity.clone(),
                })
                .collect::<Vec<_>>();

            prove_many(&actor, sub_request_keys, sub_request_entities)
                .await
                .map(|statuses| {
                    let is_all_sub_success = statuses
                        .iter()
                        .all(|status| matches!(status, raiko_reqpool::Status::Success { .. }));
                    if !is_all_sub_success {
                        raiko_reqpool::Status::WorkInProgress
                    } else {
                        raiko_reqpool::Status::Success {
                            // NOTE: Return the proof of the first sub-request
                            proof: {
                                if let raiko_reqpool::Status::Success { proof, .. } = &statuses[0] {
                                    proof.clone()
                                } else {
                                    Proof::default()
                                }
                            },
                        }
                    }
                })
        }
    };
    tracing::debug!("Batch proof result: {}", serde_json::to_string(&result)?);
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
