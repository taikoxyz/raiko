use raiko_reqactor::{Action, Gateway};
use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, Pool, RequestEntity, RequestKey,
    SingleProofRequestEntity, SingleProofRequestKey, Status,
};

pub async fn prove<P: Pool>(
    gateway: &Gateway<P>,
    request_key: RequestKey,
    request_entity: RequestEntity,
) -> Result<Status, String> {
    // Return if system is paused
    check_system_paused(&gateway)?;
    debug_request_status(&gateway, &request_key.clone().into());

    // Return if request is already completed successfully
    let status_opt = gateway.pool_get_status(&request_key.clone().into())?;
    if let Some(Status::Success { proof }) = status_opt.map(|status| status.into_status()) {
        return Ok(Status::Success { proof });
    }

    // Send the request to the Actor and return the response status
    gateway
        .send(Action::Prove {
            request_key: request_key.into(),
            request_entity: request_entity.into(),
        })
        .await
        .map(|status| status.into_status())
}

pub async fn cancel<P: Pool>(
    gateway: &Gateway<P>,
    request_key: RequestKey,
) -> Result<Status, String> {
    gateway
        .send(Action::Cancel { request_key })
        .await
        .map(|status| status.into_status())
}

pub async fn prove_aggregation<P: Pool>(
    gateway: &Gateway<P>,
    request_key: AggregationRequestKey,
    request_entity_without_proofs: AggregationRequestEntity,
    sub_request_keys: Vec<SingleProofRequestKey>,
    sub_request_entities: Vec<SingleProofRequestEntity>,
) -> Result<Status, String> {
    // Return if system is paused
    check_system_paused(&gateway)?;
    debug_request_status(&gateway, &request_key.clone().into());

    // Return if request is already completed successfully
    let status_opt = gateway.pool_get_status(&request_key.clone().into())?;
    if let Some(Status::Success { proof }) = status_opt.map(|status| status.into_status()) {
        return Ok(Status::Success { proof });
    }

    // Send the sub-requests to the Actor
    let mut statuses = Vec::with_capacity(sub_request_keys.len());
    for (sub_request_key, sub_request_entity) in
        sub_request_keys.into_iter().zip(sub_request_entities)
    {
        let status = prove(gateway, sub_request_key.into(), sub_request_entity.into()).await?;
        statuses.push(status);
    }

    let is_all_sub_success = statuses
        .iter()
        .all(|status| matches!(status, Status::Success { .. }));
    if !is_all_sub_success {
        return Ok(Status::Registered);
    }

    // Build the aggregation request entity and send the aggregation request to the Actor,
    // and return the response status
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

    prove(gateway, request_key.into(), request_entity.into()).await
}

pub async fn cancel_aggregation<P: Pool>(
    gateway: &Gateway<P>,
    request_key: AggregationRequestKey,
    sub_request_keys: Vec<SingleProofRequestKey>,
) -> Result<Status, String> {
    // Return if system is paused
    check_system_paused(&gateway)?;
    debug_request_status(&gateway, &request_key.clone().into());

    // Return if request is already cancelled
    let status_opt = gateway.pool_get_status(&request_key.clone().into())?;
    if let Some(Status::Cancelled) = status_opt.map(|status| status.into_status()) {
        return Ok(Status::Cancelled);
    }

    // Cancel the sub-requests to the Actor
    for sub_request_key in sub_request_keys {
        let _ = cancel(gateway, sub_request_key.into()).await?;
    }

    cancel(gateway, request_key.into()).await
}

fn check_system_paused<P: Pool>(gateway: &Gateway<P>) -> Result<(), String> {
    if gateway.is_paused() {
        return Err("System is paused".to_string());
    }
    Ok(())
}

fn debug_request_status<P: Pool>(gateway: &Gateway<P>, request_key: &RequestKey) {
    if let Ok(status_opt) = gateway.pool_get_status(request_key) {
        tracing::debug!("Status of request {request_key}: {status_opt:?}");
    }
}
