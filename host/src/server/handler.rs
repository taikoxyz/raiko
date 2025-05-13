use raiko_reqactor::{Action, Actor};
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, RequestEntity, RequestKey,
    SingleProofRequestKey, Status,
};
use tracing::error;

const PROOF_TIMEOUT_SEC: u64 = 20 * 60; // 20 minutes
const PROOF_WAIT_INTERVAL_SEC: u64 = 30; // 30 seconds

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

/// Prove the request and wait for the result.
pub async fn wait_prove(
    actor: &Actor,
    request_key: RequestKey,
    request_entity: RequestEntity,
) -> Result<Status, String> {
    let action = Action::Prove {
        request_key,
        request_entity,
    };

    let timeout = tokio::time::Duration::from_secs(PROOF_TIMEOUT_SEC);
    let interval = tokio::time::Duration::from_secs(PROOF_WAIT_INTERVAL_SEC);
    let start_time = tokio::time::Instant::now();

    while start_time.elapsed() < timeout {
        let status = act(actor, action.clone()).await?;
        if status.is_done() {
            return Ok(status);
        }

        tokio::time::sleep(interval).await;
    }

    error!("Timeout while waiting for proof");
    Err("Timeout while waiting for proof".to_string())
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
    let request_entity = AggregationRequestEntity::new(
        request_entity_without_proofs.aggregation_ids().clone(),
        proofs,
        request_entity_without_proofs.proof_type().clone(),
        request_entity_without_proofs.prover_args().clone(),
    );
    prove(actor, request_key.into(), request_entity.into()).await
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

// === Helper functions ===

// Send the action to the Actor and return the response status
async fn act(actor: &Actor, action: Action) -> Result<Status, String> {
    // Check if the system is paused
    if actor.is_paused() {
        return Err("System is paused".to_string());
    }

    // Return early if the request is already succeeded
    if let Ok(Some(status)) = actor.pool_get_status(&action.request_key()) {
        if matches!(status.status(), Status::Success { .. }) {
            return Ok(status.into_status());
        }
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
