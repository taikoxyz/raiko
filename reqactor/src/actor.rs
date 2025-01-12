use std::time::Duration;

use raiko_core::{
    interfaces::{aggregate_proofs, ProofRequest},
    provider::rpc::RpcBlockDataProvider,
    Raiko,
};
use raiko_lib::{
    consts::SupportedChainSpecs,
    input::{AggregationGuestInput, AggregationGuestOutput},
    prover::{IdStore, IdWrite, Proof},
};
use raiko_reqpool::{
    AggregationRequestEntity, RequestEntity, RequestKey, SingleProofRequestEntity, Status,
    StatusWithContext,
};
use reth_primitives::B256;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};

use crate::{Action, Pool};

#[derive(Clone)]
pub struct Actor<P: Pool + IdStore + IdWrite + 'static> {
    pool: P,
    chain_specs: SupportedChainSpecs,
    internal_tx: Sender<RequestKey>,
}

// TODO: load pool and notify internal channel
impl<P: Pool + IdStore + 'static> Actor<P> {
    /// Start the actor, return the actor and the sender.
    ///
    /// The returned channel sender is used to send actions to the actor, and the actor will
    /// act on the actions and send responses back.
    pub async fn start(
        pool: P,
        chain_specs: SupportedChainSpecs,
    ) -> (
        Sender<(Action, oneshot::Sender<Result<StatusWithContext, String>>)>,
        Sender<()>,
    ) {
        let channel_size = 1024;
        let (external_tx, external_rx) = mpsc::channel::<(
            Action,
            oneshot::Sender<Result<StatusWithContext, String>>,
        )>(channel_size);
        let (internal_tx, internal_rx) = mpsc::channel::<RequestKey>(channel_size);
        let (pause_tx, pause_rx) = mpsc::channel::<()>(1);

        tokio::spawn(async move {
            Actor {
                pool,
                chain_specs,
                internal_tx,
            }
            .serve(external_rx, internal_rx, pause_rx)
            .await;
        });

        (external_tx, pause_tx)
    }

    async fn serve(
        mut self,
        mut external_rx: Receiver<(Action, oneshot::Sender<Result<StatusWithContext, String>>)>,
        mut internal_rx: Receiver<RequestKey>,
        mut pause_rx: Receiver<()>,
    ) {
        loop {
            tokio::select! {
                Some((action, resp_tx)) = external_rx.recv() => {
                    let response = self.handle_external_action(action.clone()).await;
                    if let Err(err) = resp_tx.send(response.clone()) {
                        tracing::error!(
                            "Actor failed to send response {response:?} to action {action}: {err:?}"
                        );
                    }
                }
                Some(request_key) = internal_rx.recv() => {
                    if let Err(err) = self.handle_internal_signal(request_key.clone()).await {
                        tracing::error!(
                            "Actor failed to handle internal signal {request_key}: {err:?}"
                        );
                    }
                }
                Some(()) = pause_rx.recv() => {
                    tracing::info!("Actor received pause-signal, halting");
                    if let Err(err) = self.halt().await {
                        tracing::error!("Actor failed to halt: {err:?}");
                    }
                }
                else => {
                    // All channels are closed, exit the loop
                    tracing::info!("Actor exited");
                    break;
                }
            }
        }
    }

    async fn handle_external_action(
        &mut self,
        action: Action,
    ) -> Result<StatusWithContext, String> {
        match action {
            Action::Prove {
                request_key,
                request_entity,
            } => match self.pool.get_status(&request_key) {
                Ok(None) => {
                    tracing::info!("Actor received prove-action {request_key}, and it is not in pool, registering");
                    self.register(request_key, request_entity).await
                }
                Ok(Some(status)) => match status.status() {
                    Status::Registered | Status::WorkInProgress | Status::Success { .. } => {
                        tracing::info!("Actor received prove-action {request_key}, but it is already {status}, skipping");
                        Ok(status)
                    }
                    Status::Cancelled { .. } => {
                        tracing::warn!("Actor received prove-action {request_key}, and it is cancelled, re-registering");
                        self.register(request_key, request_entity).await
                    }
                    Status::Failed { .. } => {
                        tracing::warn!("Actor received prove-action {request_key}, and it is failed, re-registering");
                        self.register(request_key, request_entity).await
                    }
                },
                Err(err) => {
                    tracing::error!(
                        "Actor failed to get status of prove-action {request_key}: {err:?}"
                    );
                    Err(err)
                }
            },
            Action::Cancel { request_key } => match self.pool.get_status(&request_key) {
                Ok(None) => {
                    tracing::warn!("Actor received cancel-action {request_key}, but it is not in pool, skipping");
                    Err(format!("request {request_key} is not in pool"))
                }
                Ok(Some(status)) => match status.status() {
                    Status::Registered | Status::WorkInProgress => {
                        tracing::info!("Actor received cancel-action {request_key}, and it is {status}, cancelling");
                        self.cancel(request_key).await
                    }

                    Status::Failed { .. } | Status::Cancelled { .. } | Status::Success { .. } => {
                        tracing::info!("Actor received cancel-action {request_key}, but it is already {status}, skipping");
                        Ok(status)
                    }
                },
                Err(err) => {
                    tracing::error!(
                        "Actor failed to get status of cancel-action {request_key}: {err:?}"
                    );
                    Err(err)
                }
            },
        }
    }

    // TODO: semaphore
    async fn handle_internal_signal(&mut self, request_key: RequestKey) -> Result<(), String> {
        match self.pool.get(&request_key) {
            Ok(Some((request_entity, status))) => match status.status() {
                Status::Registered => match request_entity {
                    RequestEntity::SingleProof(entity) => {
                        self.prove_single(request_key, entity).await
                    }
                    RequestEntity::Aggregation(entity) => {
                        self.prove_aggregation(request_key, entity).await
                    }
                },
                Status::WorkInProgress => {
                    // Wait for proving completion
                    tracing::info!(
                        "Actor wait for proving completion: {request_key}, elapsed: {elapsed:?}",
                        elapsed = chrono::Utc::now() - status.timestamp(),
                    );

                    self.internal_signal_timeout(&request_key, Duration::from_secs(3))
                        .await;
                    Ok(())
                }
                Status::Success { .. } | Status::Cancelled { .. } | Status::Failed { .. } => Ok(()),
            },
            Ok(None) => {
                tracing::warn!(
                    "Actor received internal signal {request_key}, but it is not in pool, skipping"
                );
                Ok(())
            }
            Err(err) => {
                tracing::error!(
                    "Actor failed to get status of internal signal {request_key}: {err:?}, retrying"
                );

                self.internal_signal_timeout(&request_key, Duration::from_secs(3))
                    .await;
                Err(err)
            }
        }
    }

    // Resignal the request key to the internal channel after 3 seconds
    async fn internal_signal_timeout(&mut self, request_key: &RequestKey, duration: Duration) {
        // Re-signal the request key to the internal channel after 3 seconds
        let mut timer = tokio::time::interval(duration);
        let internal_tx = self.internal_tx.clone();
        let request_key = request_key.clone();
        tokio::spawn(async move {
            timer.tick().await;
            if let Err(err) = internal_tx.send(request_key.clone()).await {
                tracing::error!(
                    "Actor failed to send internal signal {request_key}: {err:?}, actor will exit"
                );
            }
        });
    }

    // Register a new request to the pool and notify the actor.
    async fn register(
        &mut self,
        request_key: RequestKey,
        request_entity: RequestEntity,
    ) -> Result<StatusWithContext, String> {
        let status = StatusWithContext::new_registered();
        if let Err(err) = self
            .pool
            .add(request_key.clone(), request_entity, status.clone())
        {
            return Err(err);
        }

        if let Err(err) = self.internal_tx.send(request_key.clone()).await {
            tracing::error!(
                "Actor failed to send internal signal {request_key}: {err:?}, actor will exit"
            );
            return Err(format!(
                "failed to send internal signal {request_key}: {err:?}, actor will exit"
            ));
        }

        Ok(status)
    }

    async fn cancel(&mut self, request_key: RequestKey) -> Result<StatusWithContext, String> {
        let Some(status) = self.pool.get_status(&request_key)? else {
            // the request is not in the pool, do nothing
            tracing::warn!(
                "Actor received cancel-action {request_key}, but it is not in pool, skipping"
            );
            return Err(format!("request {request_key} is not in pool"));
        };

        if status.status() != &Status::WorkInProgress {
            // the request is not in proving, do nothing
            tracing::warn!(
                "Actor received cancel-action {request_key}, but it is not in work-in-progress, skipping"
            );
            return Err(format!("request {request_key} is not in work-in-progress"));
        }

        tracing::info!("Actor received cancel-action {request_key}, status: {status}, cancelling");

        match &request_key {
            RequestKey::SingleProof(key) => {
                // Cancel Single Proof
                //
                // 1. Cancel the proving work by the cancel token // TODO: cancel token
                // 2. Remove the proof id from the pool
                raiko_core::interfaces::cancel_proof(
                    key.proof_type().clone(),
                    (
                        key.chain_id().clone(),
                        key.block_number().clone(),
                        key.block_hash().clone(),
                        *key.proof_type() as u8,
                    ),
                    Box::new(&mut self.pool),
                )
                .await
                .or_else(|e| {
                    if e.to_string().contains("No data for query") {
                        tracing::warn!("Actor received cancel-action {request_key}, but it is already cancelled or not yet started, skipping");
                        Ok(())
                    } else {
                        tracing::error!(
                            "Actor received cancel-action {request_key}, but failed to cancel proof: {e:?}"
                        );
                        Err(format!("failed to cancel proof: {e:?}"))
                    }
                })?;

                // 3. Mark the request as cancelled in the pool
                let status = StatusWithContext::new_cancelled();
                self.pool.update_status(request_key, status.clone())?;

                Ok(status)
            }
            RequestKey::Aggregation(..) => {
                // Cancel Aggregation Proof
                //
                // 1. Cancel the proving work by the cancel token // TODO
                // 2. Remove the proof id from the pool // TODO

                // 3. Mark the request as cancelled in the pool
                let status = StatusWithContext::new_cancelled();
                self.pool.update_status(request_key, status.clone())?;

                Ok(status)
            }
        }
    }

    async fn prove_single(
        &mut self,
        request_key: RequestKey,
        request_entity: SingleProofRequestEntity,
    ) -> Result<(), String> {
        // 1. Update the request status in pool to WorkInProgress
        self.pool
            .update_status(request_key.clone(), Status::WorkInProgress.into())?;

        let mut actor = self.clone();
        tokio::spawn(async move {
            // 2. Start the proving work
            let proven_status = do_prove_single(
                &mut actor.pool,
                &actor.chain_specs,
                request_key.clone(),
                request_entity,
            )
            .await
            .map(|proof| Status::Success { proof })
            .unwrap_or_else(|error| Status::Failed { error });

            // 3. Update the request status in pool to the resulted status
            if let Err(err) = actor
                .pool
                .update_status(request_key.clone(), proven_status.clone().into())
            {
                tracing::error!(
                    "Actor failed to update status of prove-action {request_key}: {err:?}, status: {proven_status}"
                );
                return;
            }

            // 4. Resignal the request key to the internal channel, to let the actor know the proving is done // TODO
            let _ = actor.internal_tx.send(request_key.clone()).await;
        });

        Ok(())
    }

    async fn prove_aggregation(
        &mut self,
        request_key: RequestKey,
        request_entity: AggregationRequestEntity,
    ) -> Result<(), String> {
        // 1. Update the request status in pool to WorkInProgress
        self.pool
            .update_status(request_key.clone(), Status::WorkInProgress.into())?;

        let mut actor = self.clone();
        tokio::spawn(async move {
            // 2. Start the proving work
            let proven_status =
                do_prove_aggregation(&mut actor.pool, request_key.clone(), request_entity)
                    .await
                    .map(|proof| Status::Success { proof })
                    .unwrap_or_else(|error| Status::Failed { error });

            // 3. Update the request status in pool to the resulted status
            if let Err(err) = actor
                .pool
                .update_status(request_key.clone(), proven_status.clone().into())
            {
                tracing::error!(
                    "Actor failed to update status of prove-action {request_key}: {err:?}, status: {proven_status}"
                );
                return;
            }

            // 4. Resignal the request key to the internal channel, to let the actor know the proving is done // TODO
            let _ = actor.internal_tx.send(request_key.clone()).await;
        });

        Ok(())
    }

    async fn halt(&mut self) -> Result<(), String> {
        todo!("halt")
    }
}

// TODO: cache input, reference to raiko_host::cache
// TODO: memory tracking
// TODO: metrics
// TODO: measurement
pub async fn do_prove_single(
    pool: &mut dyn IdWrite,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: SingleProofRequestEntity,
) -> Result<Proof, String> {
    tracing::info!("Generating proof for {request_key}");

    let l1_chain_spec = chain_specs
        .get_chain_spec(&request_entity.l1_network())
        .ok_or_else(|| {
            format!(
                "unsupported l1 network: {}, it should not happen, please issue a bug report",
                request_entity.l1_network()
            )
        })?;
    let taiko_chain_spec = chain_specs
        .get_chain_spec(&request_entity.network())
        .ok_or_else(|| {
            format!(
                "unsupported raiko network: {}, it should not happen, please issue a bug report",
                request_entity.network()
            )
        })?;
    let proof_request = ProofRequest {
        block_number: *request_entity.block_number(),
        l1_inclusion_block_number: *request_entity.l1_inclusion_block_number(),
        network: request_entity.network().clone(),
        l1_network: request_entity.l1_network().clone(),
        graffiti: request_entity.graffiti().clone(),
        prover: request_entity.prover().clone(),
        proof_type: request_entity.proof_type().clone(),
        blob_proof_type: request_entity.blob_proof_type().clone(),
        prover_args: request_entity.prover_args().clone(),
    };
    let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec.clone(), proof_request);
    let provider = RpcBlockDataProvider::new(
        &taiko_chain_spec.rpc.clone(),
        request_entity.block_number() - 1,
    )
    .map_err(|err| format!("failed to create rpc block data provider: {err:?}"))?;

    // 1. Generate the proof input
    let input = raiko
        .generate_input(provider)
        .await
        .map_err(|e| format!("failed to generate input: {e:?}"))?;

    // 2. Generate the proof output
    let output = raiko
        .get_output(&input)
        .map_err(|e| format!("failed to get output: {e:?}"))?;

    // 3. Generate the proof
    let proof = raiko
        .prove(input, &output, Some(pool))
        .await
        .map_err(|err| format!("failed to generate single proof: {err:?}"))?;

    Ok(proof)
}

async fn do_prove_aggregation(
    pool: &mut dyn IdWrite,
    request_key: RequestKey,
    request_entity: AggregationRequestEntity,
) -> Result<Proof, String> {
    let proof_type = request_key.proof_type().clone();
    let proofs = request_entity.proofs().clone();

    let input = AggregationGuestInput { proofs };
    let output = AggregationGuestOutput { hash: B256::ZERO };
    let config = serde_json::to_value(request_entity.prover_args())
        .map_err(|err| format!("failed to serialize prover args: {err:?}"))?;

    let proof = aggregate_proofs(proof_type, input, &output, &config, Some(pool))
        .await
        .map_err(|err| format!("failed to generate aggregation proof: {err:?}"))?;

    Ok(proof)
}
