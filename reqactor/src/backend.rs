use std::sync::Arc;

use raiko_core::{
    interfaces::{aggregate_shasta_proposals, ProofRequest},
    preflight::parse_l1_batch_proposal_tx_for_shasta_fork,
    provider::rpc::RpcBlockDataProvider,
    Raiko,
};
use raiko_lib::{
    consts::SupportedChainSpecs,
    input::{
        AggregationGuestOutput, GuestBatchInput, ShastaAggregationGuestInput,
    },
    prover::{IdWrite, Proof},
    utils::shasta_guest_input::{
        decode_guest_input_from_prover_arg_value, encode_guest_input_to_compress_b64_str,
        PROVER_ARG_SHASTA_GUEST_INPUT,
    },
};
use raiko_reqpool::{
    AggregationRequestEntity, RequestEntity, RequestKey, ShastaInputRequestEntity,
    ShastaProofRequestEntity, Status, StatusWithContext,
};
use reth_primitives::B256;
use tokio::sync::{mpsc, Mutex, Notify, Semaphore};
use tracing::trace;

use crate::queue::Queue;
use crate::Pool;

/// Backend runs in the background, and handles the actions from the actor.
#[derive(Clone, Debug)]
pub(crate) struct Backend {
    pool: Pool,
    chain_specs: SupportedChainSpecs,
    queue: Arc<Mutex<Queue>>,
    notifier: Arc<Notify>,
    semaphore: Arc<Semaphore>,
}

impl Backend {
    pub fn new(
        pool: Pool,
        chain_specs: SupportedChainSpecs,
        max_proving_concurrency: usize,
        queue: Arc<Mutex<Queue>>,
        notifier: Arc<Notify>,
    ) -> Self {
        Self {
            pool,
            chain_specs,
            queue,
            notifier,
            semaphore: Arc::new(Semaphore::new(max_proving_concurrency)),
        }
    }

    pub async fn serve_in_background(self) {
        let (done_tx, mut done_rx) = mpsc::channel(1000);

        loop {
            // Handle completed requests
            while let Ok(request_key) = done_rx.try_recv() {
                let mut queue = self.queue.lock().await;
                queue.complete(request_key);
            }

            // First, acquire a semaphore permit before choosing the next job
            let permit = match self.semaphore.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    tracing::warn!("Semaphore closed; stopping backend loop");
                    break;
                }
            };

            // Then, try to get a request from queue
            let (request_key, request_entity) = {
                let mut queue = self.queue.lock().await;
                if let Some((request_key, request_entity)) = queue.try_next() {
                    (request_key, request_entity)
                } else {
                    drop(queue);
                    // No requests available, release the permit and wait for work
                    drop(permit);
                    // No requests in queue, wait for new requests
                    self.notifier.notified().await;
                    continue;
                }
            };

            let request_key_ = request_key.clone();
            let mut pool_ = self.pool.clone();
            let chain_specs = self.chain_specs.clone();
            let handle = tokio::spawn(async move {
                let _permit = permit;

                let result = dispatch_proof_request(
                    &mut pool_,
                    &chain_specs,
                    request_key_.clone(),
                    request_entity,
                )
                .await;
                let status = result_to_status(&result, &request_key_);
                let _ = pool_.update_status(
                    request_key_.clone(),
                    StatusWithContext::new(status, chrono::Utc::now()),
                );
            });

            let mut pool_ = self.pool.clone();
            let done_tx_ = done_tx.clone();
            let notifier_ = self.notifier.clone();

            tokio::spawn(async move {
                if let Err(e) = handle.await {
                    tracing::error!("Actor thread errored while proving {request_key}: {e:?}");
                    let status =
                        StatusWithContext::new(Status::Failed { error: e.to_string() }, chrono::Utc::now());
                    let _ = pool_.update_status(request_key.clone(), status);
                }
                let _ = done_tx_.send(request_key).await;
                notifier_.notify_one();
            });
        }
    }
}

/// Dispatches the request to the appropriate proof handler.
async fn dispatch_proof_request(
    pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: RequestEntity,
) -> Result<raiko_lib::prover::Proof, String> {
    match request_entity {
        RequestEntity::SingleProof(_)
        | RequestEntity::Aggregation(_)
        | RequestEntity::BatchProof(_)
        | RequestEntity::GuestInput(_)
        | RequestEntity::BatchGuestInput(_) => Err(
            "legacy single-block and batch proving are removed; use Shasta only".to_string(),
        ),
        RequestEntity::ShastaGuestInput(entity) => {
            do_generate_shasta_proposal_guest_input(pool, chain_specs, request_key, entity).await
        }
        RequestEntity::ShastaProof(entity) => {
            do_prove_shasta_proposal(pool, chain_specs, request_key, entity).await
        }
        RequestEntity::ShastaAggregation(entity) => {
            do_shasta_aggregation(pool, request_key, entity).await
        }
    }
}

/// Converts proof result to pool status.
fn result_to_status(
    result: &Result<raiko_lib::prover::Proof, String>,
    request_key: &RequestKey,
) -> Status {
    match result {
        Ok(proof) => {
            tracing::info!("Actor Backend successfully proved {request_key}. Proof: {proof}");
            Status::Success {
                proof: proof.clone(),
            }
        }
        Err(e) => Status::Failed {
            error: e.clone(),
        },
    }
}

async fn do_shasta_aggregation(
    pool: &mut dyn IdWrite,
    request_key: RequestKey,
    request_entity: AggregationRequestEntity,
) -> Result<Proof, String> {
    let proof_type = request_key.proof_type().clone();
    let proofs = request_entity.proofs().clone();

    let input = ShastaAggregationGuestInput { proofs };
    let output = AggregationGuestOutput { hash: B256::ZERO };
    let config = serde_json::to_value(request_entity.prover_args())
        .map_err(|err| format!("failed to serialize prover args: {err:?}"))?;

    let proof = aggregate_shasta_proposals(proof_type, input, &output, &config, Some(pool))
        .await
        .map_err(|err| format!("failed to generate aggregation proof: {err:?}"))?;

    Ok(proof)
}

async fn generate_input_for_batch(raiko: &Raiko) -> Result<GuestBatchInput, String> {
    let provider_target_blocks = (raiko.request.l2_block_numbers[0] - 1
        ..=*raiko.request.l2_block_numbers.last().unwrap())
        .collect();
    let provider =
        RpcBlockDataProvider::new_batch(&raiko.taiko_chain_spec.rpc, provider_target_blocks)
            .await
            .expect("Could not create RpcBlockDataProvider");
    let input = raiko
        .generate_batch_input(provider)
        .await
        .map_err(|e| format!("failed to generate batch input: {e:?}"))?;
    Ok(input)
}

pub async fn do_generate_shasta_proposal_guest_input(
    _pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: ShastaInputRequestEntity,
) -> Result<Proof, String> {
    trace!("generate shasta guest input for: {request_key:?}");
    let shasta_proposal_request_entity: ShastaProofRequestEntity =
        ShastaProofRequestEntity::new_with_guest_input_entity(
            request_entity.clone(),
            Default::default(),
            Default::default(),
        );
    let raiko = new_raiko_for_shasta_proposal_request(chain_specs, shasta_proposal_request_entity)
        .await
        .map_err(|err| format!("failed to create raiko: {err:?}"))?;
    let input = generate_input_for_batch(&raiko)
        .await
        .map_err(|err| format!("failed to generate batch guest input: {err:?}"))?;
    let compressed_b64 = encode_guest_input_to_compress_b64_str(&input)?;
    tracing::debug!(
        "redis guest input: compressed_b64 {} bytes.",
        compressed_b64.len()
    );
    Ok(Proof {
        proof: Some(compressed_b64),
        ..Default::default()
    })
}

//for shasta proposal request
async fn new_raiko_for_shasta_proposal_request(
    chain_specs: &SupportedChainSpecs,
    request_entity: ShastaProofRequestEntity,
) -> Result<Raiko, String> {
    let l1_chain_spec = chain_specs
        .get_chain_spec(&request_entity.guest_input_entity().l1_network())
        .expect("unsupported l1 network");
    let taiko_chain_spec = chain_specs
        .get_chain_spec(&request_entity.guest_input_entity().network())
        .expect("unsupported taiko network");
    let proposal_id = request_entity.guest_input_entity().proposal_id();
    let l1_include_block_number = request_entity
        .guest_input_entity()
        .l1_inclusion_block_number();

    // parse & verify proposal event and cache it to avoid duplicate RPC calls
    let (_block_numbers, cached_event_data) = parse_l1_batch_proposal_tx_for_shasta_fork(
        &l1_chain_spec,
        &taiko_chain_spec,
        *l1_include_block_number,
        *proposal_id,
    )
    .await
    .map_err(|err| format!("Could not parse L1 shasta proposal tx: {err:?}"))?;

    let proof_request = ProofRequest {
        block_number: 0,
        batch_id: *request_entity.guest_input_entity().proposal_id(),
        l1_inclusion_block_number: *request_entity
            .guest_input_entity()
            .l1_inclusion_block_number(),
        network: request_entity.guest_input_entity().network().clone(),
        l1_network: request_entity.guest_input_entity().l1_network().clone(),
        graffiti: Default::default(),
        prover: request_entity.guest_input_entity().actual_prover().clone(),
        proof_type: request_entity.proof_type().clone(),
        blob_proof_type: request_entity
            .guest_input_entity()
            .blob_proof_type()
            .clone(),
        prover_args: request_entity.prover_args().clone(),
        l2_block_numbers: request_entity.guest_input_entity().l2_blocks().clone(),
        checkpoint: request_entity.guest_input_entity().checkpoint().clone(),
        last_anchor_block_number: Some(
            request_entity
                .guest_input_entity()
                .last_anchor_block_number()
                .clone(),
        ),
        cached_event_data: Some(cached_event_data),
    };

    Ok(Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request))
}

pub async fn do_prove_shasta_proposal(
    _pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: ShastaProofRequestEntity,
) -> Result<Proof, String> {
    tracing::info!("generate shasta proposal proof for: {request_key:?}");

    let raiko = new_raiko_for_shasta_proposal_request(chain_specs, request_entity.clone())
        .await
        .map_err(|err| format!("failed to create raiko: {err:?}"))?;

    let input = if let Some(shasta_guest_input) =
        raiko.request.prover_args.get(PROVER_ARG_SHASTA_GUEST_INPUT)
    {
        decode_guest_input_from_prover_arg_value(shasta_guest_input)?
    } else {
        tracing::warn!("rebuild shasta guest input for request: {request_key:?}");
        generate_input_for_batch(&raiko)
            .await
            .map_err(|err| format!("failed to generate shasta guest input: {err:?}"))?
    };

    // Generate the output for the batch
    let output = raiko
        .get_batch_output(&input)
        .map_err(|err| format!("failed to generate output: {err:?}"))?;

    // Run the Shasta proposal prover
    let proof = raiko
        .shasta_proposal_prove(input, &output, None)
        .await
        .map_err(|err| format!("failed to run shasta proposal prover: {err:?}"))?;

    Ok(proof)
}

#[cfg(test)]
mod tests {
    use super::*;
    use raiko_lib::consts::SupportedChainSpecs;
    use raiko_reqpool::memory_pool;
    use tokio::sync::Mutex;

    fn create_test_pool() -> Pool {
        memory_pool("test_backend")
    }

    fn create_test_chain_specs() -> SupportedChainSpecs {
        SupportedChainSpecs::default()
    }

    // Mock test for the serve_in_background to test the structure.
    #[tokio::test]
    async fn test_serve_in_background() {
        let pool = create_test_pool();
        let chain_specs = create_test_chain_specs();
        let queue = Arc::new(Mutex::new(Queue::new(1000)));
        let notifier = Arc::new(Notify::new());

        let backend = Backend::new(pool, chain_specs, 1, queue.clone(), notifier.clone());

        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = backend.serve_in_background() => {},
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(10)) => {
                }
            }
        });

        // Notify to wake up the background service
        notifier.notify_one();

        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        handle.abort();

        assert!(true);
    }
}
