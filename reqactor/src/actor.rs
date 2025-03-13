use crate::{actor_inner::ActorInner, Action};
use raiko_ballot::Ballot;
use raiko_core::{
    interfaces::{aggregate_proofs, ProofRequest, ProofRequestOpt},
    preflight::parse_l1_batch_proposal_tx_for_pacaya_fork,
    provider::rpc::RpcBlockDataProvider,
    Raiko,
};
use raiko_lib::{
    consts::{ChainSpec, SupportedChainSpecs},
    input::{AggregationGuestInput, AggregationGuestOutput},
    proof_type::ProofType,
    prover::{IdWrite, Proof},
};
use raiko_reqpool::{
    AggregationRequestEntity, BatchProofRequestEntity, Pool, RequestEntity, RequestKey,
    SingleProofRequestEntity, Status, StatusWithContext,
};
use reth_primitives::{BlockHash, B256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio::sync::{Mutex, Notify};

/// Actor is the main interface interacting with the backend and the pool.
#[derive(Clone)]
pub struct Actor {
    default_request_config: ProofRequestOpt,
    chain_specs: SupportedChainSpecs,
    is_paused: Arc<AtomicBool>,

    // TODO: Remove Mutex. currently, in order to pass `&mut Pool`, we need to use Arc<Mutex<Pool>>.
    pool: Arc<Mutex<Pool>>,
    // In order to support dynamic config via HTTP, we need to use Arc<Mutex<Ballot>>.
    ballot: Arc<Mutex<Ballot>>,

    inner: Arc<Mutex<ActorInner>>,
    notifier: Arc<Notify>,
}

impl Actor {
    pub fn new(
        pool: Pool,
        ballot: Ballot,
        default_request_config: ProofRequestOpt,
        chain_specs: SupportedChainSpecs,
    ) -> Self {
        Self {
            default_request_config,
            chain_specs,
            is_paused: Arc::new(AtomicBool::new(false)),
            ballot: Arc::new(Mutex::new(ballot)),
            pool: Arc::new(Mutex::new(pool)),

            inner: Arc::new(Mutex::new(ActorInner::new())),
            notifier: Arc::new(Notify::new()),
        }
    }

    /// Return the default request config.
    pub fn default_request_config(&self) -> &ProofRequestOpt {
        &self.default_request_config
    }

    /// Return the chain specs.
    pub fn chain_specs(&self) -> &SupportedChainSpecs {
        &self.chain_specs
    }

    pub fn get_chain_spec(&self, network: &str) -> Result<ChainSpec, String> {
        self.chain_specs
            .get_chain_spec(network)
            .ok_or_else(|| format!("unsupported network: {}", network))
    }

    /// Check if the system is paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    /// Get the status of the request from the pool.
    pub async fn pool_get_status(
        &self,
        request_key: &RequestKey,
    ) -> Result<Option<StatusWithContext>, String> {
        self.pool.lock().await.get_status(request_key)
    }

    pub async fn pool_list_status(&self) -> Result<HashMap<RequestKey, StatusWithContext>, String> {
        self.pool.lock().await.list()
    }

    pub async fn pool_remove_request(&self, request_key: &RequestKey) -> Result<usize, String> {
        self.pool.lock().await.remove(request_key)
    }

    /// Return the pool_status of the action from the pool, and asynchronously send the action to the backend.
    pub async fn act(&self, action: Action) -> Result<StatusWithContext, String> {
        let request_key = action.request_key();
        let status = match self.pool_get_status(&request_key).await? {
            Some(status) => status,
            None => {
                let status = StatusWithContext::new_registered();
                let _ = self
                    .pool
                    .lock()
                    .await
                    .update_status(request_key.clone(), status.clone());
                status
            }
        };

        // push new request into the queue and notify to start the action
        let mut inner = self.inner.lock().await;
        if !inner.contains(&action) {
            inner.push(action);
            self.notifier.notify_one();
        }

        return Ok(status);
    }

    /// Set the pause flag and notify the task manager to pause, then wait for the task manager to
    /// finish the pause process.
    ///
    /// Note that this function is blocking until the task manager finishes the pause process.
    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::SeqCst);
    }

    pub async fn get_ballot(&self) -> Ballot {
        let ballot = self.ballot.lock().await;
        ballot.clone()
    }

    pub async fn set_ballot(&self, new_ballot: Ballot) {
        let mut ballot = self.ballot.lock().await;
        *ballot = new_ballot;
    }

    /// Draw proof types based on the block hash.
    pub async fn draw(&self, block_hash: &BlockHash) -> Option<ProofType> {
        let ballot = self.ballot.lock().await;
        ballot.draw(block_hash)
    }

    pub async fn serve_in_background(&self, max_concurrency: usize) {
        let semaphore = Arc::new(Semaphore::new(max_concurrency));
        let (done_tx, mut done_rx) = mpsc::channel(max_concurrency);

        loop {
            while let Ok(action) = done_rx.try_recv() {
                let mut inner = self.inner.lock().await;
                inner.remove_in_flight(&action);
            }

            let action = {
                let mut inner = self.inner.lock().await;
                let action = if let Some(action) = inner.pop() {
                    action
                } else {
                    drop(inner);
                    self.notifier.notified().await;
                    continue;
                };

                action
            };
            let request_key = action.request_key().clone();

            let pool_ = self.pool.lock().await.clone();
            let chain_specs = self.chain_specs.clone();
            let semaphore_ = semaphore.clone();
            let done_tx_ = done_tx.clone();
            let (semaphore_acquired_tx, semaphore_acquired_rx) = oneshot::channel();
            let handle = tokio::spawn(async move {
                let _permit = semaphore_.acquire().await.unwrap();
                let _ = semaphore_acquired_tx.send(());

                match action.clone() {
                    Action::Prove {
                        request_key,
                        request_entity,
                    } => match request_entity {
                        RequestEntity::SingleProof(entity) => {
                            prove_single(pool_, chain_specs, request_key, entity).await;
                        }
                        RequestEntity::Aggregation(entity) => {
                            prove_aggregation(pool_, request_key, entity).await;
                        }
                        RequestEntity::BatchProof(entity) => {
                            prove_batch(pool_, chain_specs, request_key, entity).await;
                        }
                    },
                    Action::Cancel { request_key } => {
                        let _ = cancel(pool_, request_key).await;
                    }
                }

                let _ = done_tx_.send(action);
            });

            // Wait for the semaphore to be acquired
            let _ = semaphore_acquired_rx.await;

            let mut pool_ = self.pool.lock().await.clone();
            tokio::spawn(async move {
                if let Err(e) = handle.await {
                    if e.is_panic() {
                        tracing::error!("Actor panicked while proving: {e:?}");
                        let status = Status::Failed {
                            error: e.to_string(),
                        };
                        if let Err(err) =
                            pool_.update_status(request_key.clone(), status.clone().into())
                        {
                            tracing::error!(
                                "Actor failed to update status of prove-action {request_key}: {err:?}, status: {status}",
                                status = status,
                            );
                        }
                    } else {
                        tracing::error!("Actor failed to prove: {e:?}");
                    }
                }
            });
        }
    }
}

async fn cancel(mut pool: Pool, request_key: RequestKey) -> Result<StatusWithContext, String> {
    let old_status = pool
        .get_status(&request_key)?
        .unwrap_or(StatusWithContext::new_registered());
    if old_status.status() != &Status::Registered && old_status.status() != &Status::WorkInProgress
    {
        tracing::warn!("Actor received cancel-action {request_key}, but it is not registered or work-in-progress, skipping");
        return Ok(old_status);
    }

    // Case: old_status is registered: mark the request as cancelled in the pool and return directly
    if old_status.status() == &Status::Registered {
        let status = StatusWithContext::new_cancelled();
        pool.update_status(request_key, status.clone())?;
        return Ok(status);
    }

    // Case: old_status is work-in-progress:
    // 1. Cancel the proving work by the cancel token // TODO: cancel token
    // 2. Remove the proof id from the pool
    // 3. Mark the request as cancelled in the pool
    match &request_key {
        RequestKey::SingleProof(key) => {
            raiko_core::interfaces::cancel_proof(
                    key.proof_type().clone(),
                    (
                        key.chain_id().clone(),
                        key.block_number().clone(),
                        key.block_hash().clone(),
                        *key.proof_type() as u8,
                    ),
                    Box::new(&mut pool),
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
            pool.update_status(request_key, status.clone())?;
            Ok(status)
        }
        RequestKey::Aggregation(..) => {
            let status = StatusWithContext::new_cancelled();
            pool.update_status(request_key, status.clone())?;
            Ok(status)
        }
        RequestKey::BatchProof(..) => {
            let status = StatusWithContext::new_cancelled();
            pool.update_status(request_key, status.clone())?;
            Ok(status)
        }
    }
}

async fn prove_single(
    pool: Pool,
    chain_specs: SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: SingleProofRequestEntity,
) {
    prove(
        pool,
        request_key.clone(),
        |mut pool, request_key| async move {
            do_prove_single(&mut pool, &chain_specs, request_key.clone(), request_entity).await
        },
    )
    .await;
}

async fn prove_aggregation(
    pool: Pool,
    request_key: RequestKey,
    request_entity: AggregationRequestEntity,
) {
    prove(
        pool,
        request_key.clone(),
        |mut pool, request_key| async move {
            do_prove_aggregation(&mut pool, request_key.clone(), request_entity).await
        },
    )
    .await;
}

async fn prove_batch(
    pool: Pool,
    chain_specs: SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: BatchProofRequestEntity,
) {
    prove(
        pool,
        request_key.clone(),
        |mut pool, request_key| async move {
            do_prove_batch(&mut pool, chain_specs, request_key.clone(), request_entity).await
        },
    )
    .await;
}

/// Generic method to handle proving for different types of proofs.
///
/// Note that this method will block the current thread until the proving thread acquires the
/// semaphore.
async fn prove<F, Fut>(mut pool: Pool, request_key: RequestKey, prove_fn: F)
where
    F: FnOnce(Pool, RequestKey) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<Proof, String>> + Send + 'static,
{
    // 1. Update the request status in pool to WorkInProgress
    if let Err(err) = pool.update_status(request_key.clone(), Status::WorkInProgress.into()) {
        tracing::error!(
                "Actor failed to update status of prove-action {request_key}: {err:?}, status: {status}",
                status = Status::WorkInProgress,
            );
        return;
    }

    // 2.1. Start the proving work
    let proven_status = {
        let start_time = Instant::now();
        let proven_status = prove_fn(pool.clone(), request_key.clone())
            .await
            .map(|proof| Status::Success { proof })
            .unwrap_or_else(|error| Status::Failed { error });
        raiko_metrics::observe_action_prove_duration(
            request_key.proof_type(),
            &request_key,
            &proven_status,
            start_time.elapsed(),
        );
        proven_status
    };

    match &proven_status {
        Status::Success { proof } => {
            tracing::info!("Actor successfully proved {request_key}, {:?}", proof);
        }
        Status::Failed { error } => {
            tracing::error!("Actor failed to prove {request_key}: {error}");
        }
        _ => {}
    }

    // 2.2. Update the request status in pool to the resulted status
    if let Err(err) = pool.update_status(request_key.clone(), proven_status.clone().into()) {
        tracing::error!(
                    "Actor failed to update status of prove-action {request_key}: {err:?}, status: {proven_status}"
                );
        return;
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
        batch_id: 0,
        l2_block_numbers: Vec::new(),
    };
    let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec.clone(), proof_request);
    let provider = RpcBlockDataProvider::new(
        &taiko_chain_spec.rpc.clone(),
        request_entity.block_number() - 1,
    )
    .await
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

async fn do_prove_batch(
    pool: &mut dyn IdWrite,
    chain_specs: SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: BatchProofRequestEntity,
) -> Result<Proof, String> {
    tracing::info!("Generating proof for {request_key}");

    let l1_chain_spec = chain_specs
        .get_chain_spec(&request_entity.l1_network())
        .expect("unsupported l1 network");
    let taiko_chain_spec = chain_specs
        .get_chain_spec(&request_entity.network())
        .expect("unsupported taiko network");
    let batch_id = request_entity.batch_id();
    let l1_include_block_number = request_entity.l1_inclusion_block_number();
    // parse the batch proposal tx to get all prove blocks
    let all_prove_blocks = parse_l1_batch_proposal_tx_for_pacaya_fork(
        &l1_chain_spec,
        &taiko_chain_spec,
        *l1_include_block_number,
        *batch_id,
    )
    .await
    .map_err(|err| format!("Could not parse L1 batch proposal tx: {err:?}"))?;
    // provider target blocks are all blocks in the batch and the parent block of block[0]
    let provider_target_blocks =
        (all_prove_blocks[0] - 1..=*all_prove_blocks.last().unwrap()).collect();
    let provider = RpcBlockDataProvider::new_batch(&taiko_chain_spec.rpc, provider_target_blocks)
        .await
        .expect("Could not create RpcBlockDataProvider");
    let proof_request = ProofRequest {
        block_number: 0,
        batch_id: *request_entity.batch_id(),
        l1_inclusion_block_number: *request_entity.l1_inclusion_block_number(),
        network: request_entity.network().clone(),
        l1_network: request_entity.l1_network().clone(),
        graffiti: request_entity.graffiti().clone(),
        prover: request_entity.prover().clone(),
        proof_type: request_entity.proof_type().clone(),
        blob_proof_type: request_entity.blob_proof_type().clone(),
        prover_args: request_entity.prover_args().clone(),
        l2_block_numbers: all_prove_blocks.clone(),
    };
    let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request);
    let input = raiko
        .generate_batch_input(provider)
        .await
        .map_err(|e| format!("failed to generateg guest batch input: {e:?}"))?;
    tracing::trace!("batch guest input: {input:?}");
    let output = raiko
        .get_batch_output(&input)
        .map_err(|e| format!("failed to get guest batch output: {e:?}"))?;
    tracing::debug!("batch guest output: {output:?}");
    let proof = raiko
        .batch_prove(input, &output, Some(pool))
        .await
        .map_err(|e| format!("failed to generate batch proof: {e:?}"))?;
    Ok(proof)
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use alloy_primitives::Address;
//     use raiko_lib::{
//         consts::SupportedChainSpecs,
//         input::BlobProofType,
//         primitives::{ChainId, B256},
//         proof_type::ProofType,
//     };
//     use raiko_reqpool::{
//         memory_pool, RequestEntity, RequestKey, SingleProofRequestEntity, SingleProofRequestKey,
//         StatusWithContext,
//     };
//     use std::collections::HashMap;
//     use tokio::sync::mpsc;

//     #[tokio::test]
//     async fn test_pause_sets_is_paused_flag() {
//         let (action_tx, _) = mpsc::channel(1);
//         let (pause_tx, _pause_rx) = mpsc::channel(1);

//         let pool = memory_pool("test_pause_sets_is_paused_flag");
//         let actor = Actor::new(
//             pool,
//             Ballot::default(),
//             ProofRequestOpt::default(),
//             SupportedChainSpecs::default(),
//             action_tx,
//             pause_tx,
//         );

//         assert!(!actor.is_paused(), "Actor should not be paused initially");

//         actor.pause().await.expect("Pause should succeed");
//         assert!(
//             actor.is_paused(),
//             "Actor should be paused after calling pause()"
//         );
//     }

//     #[tokio::test]
//     async fn test_act_sends_action_and_returns_response() {
//         let (action_tx, mut action_rx) = mpsc::channel(1);
//         let (pause_tx, _) = mpsc::channel(1);

//         let pool = memory_pool("test_act_sends_action_and_returns_response");
//         let actor = Actor::new(
//             pool,
//             Ballot::default(),
//             ProofRequestOpt::default(),
//             SupportedChainSpecs::default(),
//             action_tx,
//             pause_tx,
//         );

//         // Create a test action
//         let request_key = RequestKey::SingleProof(SingleProofRequestKey::new(
//             ChainId::default(),
//             1,
//             B256::default(),
//             ProofType::default(),
//             "test_prover".to_string(),
//         ));
//         let request_entity = RequestEntity::SingleProof(SingleProofRequestEntity::new(
//             1,
//             1,
//             "test_network".to_string(),
//             "test_l1_network".to_string(),
//             B256::default(),
//             Address::default(),
//             ProofType::default(),
//             BlobProofType::default(),
//             HashMap::new(),
//         ));
//         let test_action = Action::Prove {
//             request_key: request_key.clone(),
//             request_entity,
//         };

//         // Spawn a task to handle the action and send back a response
//         let status = StatusWithContext::new_registered();
//         let status_clone = status.clone();
//         let handle = tokio::spawn(async move {
//             let (action, resp_tx) = action_rx.recv().await.expect("Should receive action");
//             // Verify we received the expected action
//             assert_eq!(action.request_key(), &request_key);
//             // Send back a mock response with Registered status
//             resp_tx
//                 .send(Ok(status_clone))
//                 .expect("Should send response");
//         });

//         // Send the action and wait for response
//         let result = actor.act(test_action).await;

//         // Make sure we got back an Ok response
//         assert_eq!(result, Ok(status), "Should receive successful response");

//         // Wait for the handler to complete
//         handle.await.expect("Handler should complete");
//     }
// }
