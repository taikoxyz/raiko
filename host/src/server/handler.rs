use raiko_reqactor::{Action, Actor};
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, RequestEntity, RequestKey,
    SingleProofRequestEntity, SingleProofRequestKey, Status,
};

// NOTE: HTTP handlers should not check the status of the request, but just send the request to the Actor. In
// another word, Actor should be the only one guarding the status of the request.

/// Prove the request.
pub async fn prove(
    actor: &Actor,
    request_key: RequestKey,
    request_entity: RequestEntity,
) -> Result<Status, String> {
    let action = Action::Prove {
        request_key,
        request_entity,
    };
    act(actor, action).await
}

/// Cancel the request.
pub async fn cancel(actor: &Actor, request_key: RequestKey) -> Result<Status, String> {
    let action = Action::Cancel { request_key };
    act(actor, action).await
}

/// Prove the aggregation request and its sub-requests.
pub async fn prove_aggregation(
    actor: &Actor,
    request_key: AggregationRequestKey,
    request_entity_without_proofs: AggregationRequestEntity,
    sub_request_keys: Vec<SingleProofRequestKey>,
    sub_request_entities: Vec<SingleProofRequestEntity>,
) -> Result<Status, String> {
    // Prove the sub-requests
    let mut statuses = Vec::with_capacity(sub_request_keys.len());
    for (sub_request_key, sub_request_entity) in
        sub_request_keys.into_iter().zip(sub_request_entities)
    {
        let status = prove(actor, sub_request_key.into(), sub_request_entity.into()).await?;
        statuses.push(status);
    }

    let is_all_sub_success = statuses
        .iter()
        .all(|status| matches!(status, Status::Success { .. }));
    if !is_all_sub_success {
        tracing::info!(
            "Not all sub-requests are successful proven {request_key}, return registered"
        );
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
    let request_entity = AggregationRequestEntity::new(
        request_entity_without_proofs.aggregation_ids().clone(),
        proofs,
        request_entity_without_proofs.proof_type().clone(),
        request_entity_without_proofs.prover_args().clone(),
    );
    prove(actor, request_key.into(), request_entity.into()).await
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

// === Helper functions ===

// Send the action to the Actor and return the response status
async fn act(actor: &Actor, action: Action) -> Result<Status, String> {
    // Check if the system is paused
    if actor.is_paused() {
        return Err("System is paused".to_string());
    }

    // Just logging the status of the request
    let _ = actor
        .pool_get_status(&action.request_key())
        .map(|status_opt| {
            tracing::trace!(
                "trace request in {request_key}: {status}",
                request_key = action.request_key(),
                status = status_opt
                    .map(|status| status.into_status().to_string())
                    .unwrap_or("None".to_string()),
            )
        });

    // Send the action to the Actor and return the response status
    actor.act(action.clone()).await.map(|status| {
        tracing::trace!(
            "trace request out {request_key}: {status}",
            request_key = action.request_key(),
            status = status.status()
        );
        status.into_status()
    })
}
