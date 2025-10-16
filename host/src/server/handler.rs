use raiko_reqactor::Actor;
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, RequestEntity, RequestKey,
    SingleProofRequestKey, Status,
};

// NOTE: HTTP handlers should not check the status of the request, but just send the request to the Actor. In
// another word, Actor should be the only one guarding the status of the request.

/// Prove the request.
pub async fn prove(
    actor: &Actor,
    request_key: RequestKey,
    request_entity: RequestEntity,
) -> Result<Status, String> {
    if actor.is_paused() {
        return Err("System is paused".to_string());
    }

    actor
        .act(request_key.clone(), request_entity, chrono::Utc::now())
        .await
        .map(|status| status.into_status())
}

/// Cancel the request.
pub async fn cancel(_actor: &Actor, _request_key: RequestKey) -> Result<Status, String> {
    unimplemented!()
}

/// Prove the aggregation request and its sub-requests.
pub async fn prove_aggregation(
    actor: &Actor,
    request_key: RequestKey,
    request_entity_without_proofs: RequestEntity,
    sub_request_keys: Vec<RequestKey>,
    sub_request_entities: Vec<RequestEntity>,
) -> Result<Status, String> {
    // Prove the sub-requests
    let statuses = prove_many(actor, sub_request_keys, sub_request_entities).await?;
    let is_all_sub_success = statuses
        .iter()
        .all(|status| matches!(status, Status::Success { .. }));
    if !is_all_sub_success {
        return Ok(Status::Registered);
    }

    // Prove the aggregation request
    let proofs = statuses
        .into_iter()
        .map(|status| match status {
            Status::Success { proof } => proof,
            _ => unreachable!("checked above"),
        })
        .collect();
    let request_entity = match (&request_key, &request_entity_without_proofs) {
        (RequestKey::Aggregation(_), RequestEntity::Aggregation(entity)) => {
            RequestEntity::Aggregation(AggregationRequestEntity::new(
                entity.aggregation_ids().clone(),
                proofs,
                entity.proof_type().clone(),
                entity.prover_args().clone(),
            ))
        }
        (RequestKey::ShastaAggregation(_), RequestEntity::ShastaAggregation(entity)) => {
            RequestEntity::ShastaAggregation(AggregationRequestEntity::new(
                entity.aggregation_ids().clone(),
                proofs,
                entity.proof_type().clone(),
                entity.prover_args().clone(),
            ))
        }
        _ => unreachable!("Invalid request key"),
    };
    prove(actor, request_key, request_entity).await
}

/// Prove many requests.
pub(crate) async fn prove_many(
    actor: &Actor,
    request_keys: Vec<RequestKey>,
    request_entities: Vec<RequestEntity>,
) -> Result<Vec<Status>, String> {
    let mut statuses = Vec::with_capacity(request_keys.len());
    for (request_key, request_entity) in request_keys.into_iter().zip(request_entities) {
        match (request_key, request_entity) {
            (RequestKey::SingleProof(key), RequestEntity::SingleProof(entity)) => {
                let status = prove(actor, key.into(), entity.into()).await?;
                statuses.push(status);
            }
            (RequestKey::BatchProof(key), RequestEntity::BatchProof(entity)) => {
                let status = prove(actor, key.into(), entity.into()).await?;
                statuses.push(status);
            }
            (RequestKey::BatchGuestInput(key), RequestEntity::BatchGuestInput(entity)) => {
                let status = prove(actor, key.into(), entity.into()).await?;
                statuses.push(status);
            }
            (RequestKey::ShastaGuestInput(key), RequestEntity::ShastaGuestInput(entity)) => {
                let status = prove(actor, key.into(), entity.into()).await?;
                statuses.push(status);
            }
            (RequestKey::ShastaProof(key), RequestEntity::ShastaProof(entity)) => {
                let status = prove(actor, key.into(), entity.into()).await?;
                statuses.push(status);
            }
            (RequestKey::ShastaAggregation(key), RequestEntity::ShastaAggregation(entity)) => {
                let status = prove(actor, key.into(), entity.into()).await?;
                statuses.push(status);
            }
            _ => return Err("Invalid request key and entity".to_string()),
        }
    }

    Ok(statuses)
}

pub async fn cancel_aggregation(
    actor: &Actor,
    request_key: AggregationRequestKey,
    sub_request_keys: Vec<SingleProofRequestKey>,
) -> Result<Status, String> {
    for sub_request_key in sub_request_keys {
        let _discard = cancel(actor, sub_request_key.into()).await?;
    }
    cancel(actor, request_key.into()).await
}
