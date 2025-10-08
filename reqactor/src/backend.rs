use std::sync::Arc;

use base64::{engine::general_purpose, Engine as _};
use bincode;

use raiko_core::{
    interfaces::{aggregate_proofs, ProofRequest},
    preflight::parse_l1_batch_proposal_tx_for_pacaya_fork,
    provider::rpc::RpcBlockDataProvider,
    Raiko,
};
use raiko_lib::{
    consts::SupportedChainSpecs,
    input::{AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestInput},
    prover::{IdWrite, Proof},
    utils::{zlib_compress_data, zlib_decompress_data},
};
use raiko_reqpool::{
    AggregationRequestEntity, BatchGuestInputRequestEntity, BatchProofRequestEntity,
    GuestInputRequestEntity, RequestEntity, RequestKey, SingleProofRequestEntity, Status,
    StatusWithContext,
};
use reth_primitives::B256;
use tokio::sync::{mpsc, Mutex, Notify, Semaphore};
use tracing::{debug, trace};

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

                let result = match request_entity {
                    RequestEntity::SingleProof(entity) => {
                        do_prove_single(&mut pool_, &chain_specs, request_key_.clone(), entity)
                            .await
                    }
                    RequestEntity::Aggregation(entity) => {
                        do_prove_aggregation(&mut pool_, request_key_.clone(), entity).await
                    }
                    RequestEntity::BatchProof(entity) => {
                        do_prove_batch(&mut pool_, &chain_specs, request_key_.clone(), entity).await
                    }
                    RequestEntity::GuestInput(entity) => {
                        do_generate_guest_input(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                        )
                        .await
                    }
                    RequestEntity::BatchGuestInput(entity) => {
                        do_generate_batch_guest_input(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                        )
                        .await
                    }
                };
                let status = match result {
                    Ok(proof) => {
                        let proof_str = format!("{}", proof);
                        tracing::info!(
                            "Actor Backend successfully proved {request_key_}. Proof: {proof_str}"
                        );
                        Status::Success { proof }
                    }
                    Err(e) => Status::Failed {
                        error: e.to_string(),
                    },
                };
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
                    let status = Status::Failed {
                        error: e.to_string(),
                    };
                    let _ = pool_.update_status(request_key.clone(), status.clone().into());
                }

                let _res = done_tx_.send(request_key.clone()).await;
                notifier_.notify_one();
            });
        }
    }
}

pub async fn do_generate_guest_input(
    _pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: GuestInputRequestEntity,
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
        prover: Default::default(),
        proof_type: Default::default(),
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

    let input = raiko
        .generate_input(provider)
        .await
        .map_err(|e| format!("failed to generate input: {e:?}"))?;

    let input_proof = serde_json::to_string(&input).expect("input serialize ok");
    Ok(Proof {
        proof: Some(input_proof),
        ..Default::default()
    })
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

    // double check if we already have the guest_input
    let input: GuestInput =
        if let Some(guest_input_value) = request_entity.prover_args().get("guest_input") {
            let guest_input_json: String = serde_json::from_value(guest_input_value.clone())
                .expect("guest_input should be a string");
            let mut input: GuestInput = serde_json::from_str(&guest_input_json)
                .map_err(|err| format!("failed to deserialize guest_input: {err:?}"))?;
            // update missing fields
            let prover_data = &input.taiko.prover_data;
            if !(prover_data.graffiti.eq(request_entity.graffiti())
                && prover_data.prover.eq(request_entity.prover()))
            {
                input.taiko.prover_data = raiko_lib::input::TaikoProverData {
                    graffiti: request_entity.graffiti().clone(),
                    prover: request_entity.prover().clone(),
                }
            }
            input
        } else {
            // 1. Generate the proof input
            raiko
                .generate_input(provider)
                .await
                .map_err(|e| format!("failed to generate input: {e:?}"))?
        };

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

async fn new_raiko_for_batch_request(
    chain_specs: &SupportedChainSpecs,
    request_entity: BatchProofRequestEntity,
) -> Result<Raiko, String> {
    let l1_chain_spec = chain_specs
        .get_chain_spec(&request_entity.guest_input_entity().l1_network())
        .expect("unsupported l1 network");
    let taiko_chain_spec = chain_specs
        .get_chain_spec(&request_entity.guest_input_entity().network())
        .expect("unsupported taiko network");
    let batch_id = request_entity.guest_input_entity().batch_id();
    let l1_include_block_number = request_entity
        .guest_input_entity()
        .l1_inclusion_block_number();
    // parse the batch proposal tx to get all prove blocks
    let all_prove_blocks = parse_l1_batch_proposal_tx_for_pacaya_fork(
        &l1_chain_spec,
        &taiko_chain_spec,
        *l1_include_block_number,
        *batch_id,
    )
    .await
    .map_err(|err| format!("Could not parse L1 batch proposal tx: {err:?}"))?;

    let proof_request = ProofRequest {
        block_number: 0,
        batch_id: *request_entity.guest_input_entity().batch_id(),
        l1_inclusion_block_number: *request_entity
            .guest_input_entity()
            .l1_inclusion_block_number(),
        network: request_entity.guest_input_entity().network().clone(),
        l1_network: request_entity.guest_input_entity().l1_network().clone(),
        graffiti: request_entity.guest_input_entity().graffiti().clone(),
        prover: request_entity.prover().clone(),
        proof_type: request_entity.proof_type().clone(),
        blob_proof_type: request_entity
            .guest_input_entity()
            .blob_proof_type()
            .clone(),
        prover_args: request_entity.prover_args().clone(),
        l2_block_numbers: all_prove_blocks.clone(),
    };

    Ok(Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request))
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

pub async fn do_generate_batch_guest_input(
    _pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: BatchGuestInputRequestEntity,
) -> Result<Proof, String> {
    trace!("batch guest input for: {request_key:?}");
    let batch_proof_request_entity = BatchProofRequestEntity::new_with_guest_input_entity(
        request_entity.clone(),
        Default::default(),
        Default::default(),
        Default::default(),
    );
    let raiko = new_raiko_for_batch_request(chain_specs, batch_proof_request_entity)
        .await
        .map_err(|err| format!("failed to create raiko: {err:?}"))?;
    let input = generate_input_for_batch(&raiko)
        .await
        .map_err(|err| format!("failed to generate batch guest input: {err:?}"))?;
    let input_proof = bincode::serialize(&input)
        .map_err(|err| format!("failed to serialize input to bincode: {err:?}"))?;
    let compressed_bytes = zlib_compress_data(&input_proof).unwrap();
    let compressed_b64: String = general_purpose::STANDARD.encode(&compressed_bytes);
    tracing::debug!(
        "compress redis input: input_proof {} bytes to compressed_b64 {} bytes.",
        input_proof.len(),
        compressed_b64.len()
    );
    Ok(Proof {
        proof: Some(compressed_b64),
        ..Default::default()
    })
}

async fn do_prove_batch(
    pool: &mut dyn IdWrite,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: BatchProofRequestEntity,
) -> Result<Proof, String> {
    tracing::info!("Generating proof for {request_key}");

    let raiko = new_raiko_for_batch_request(chain_specs, request_entity).await?;
    let input = if let Some(batch_guest_input) = raiko.request.prover_args.get("batch_guest_input")
    {
        // Tricky: originally the input was created (and pass around) by prove() infra,
        // so it's a base64 string(in Proof).
        // after we get it from db somewhere before, we need to pass it down here, but there is no known
        // string carrier in key / entity, so we call deser twice, value -> string -> struct.
        let b64_encoded_string: String = serde_json::from_value(batch_guest_input.clone())
            .map_err(|err| {
                format!("failed to deserialize batch_guest_input from value: {err:?}")
            })?;
        let compressed_bytes = general_purpose::STANDARD
            .decode(&b64_encoded_string)
            .unwrap();
        let decompressed_bytes = zlib_decompress_data(&compressed_bytes)
            .map_err(|err| format!("failed to decompress batch_guest_input: {err:?}"))?;
        let guest_input: GuestBatchInput = bincode::deserialize(&decompressed_bytes)
            .map_err(|err| format!("failed to deserialize bincode batch_guest_input: {err:?}"))?;
        guest_input
    } else {
        tracing::warn!("rebuild batch guest input for request: {request_key:?}");
        generate_input_for_batch(&raiko)
            .await
            .map_err(|err| format!("failed to generate batch guest input: {err:?}"))?
    };

    let output = raiko
        .get_batch_output(&input)
        .map_err(|e| format!("failed to get guest batch output: {e:?}"))?;
    debug!("batch guest output: {output:?}");
    let proof = raiko
        .batch_prove(input, &output, Some(pool))
        .await
        .map_err(|e| format!("failed to generate batch proof: {e:?}"))?;
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use super::*;
    use raiko_lib::consts::SupportedChainSpecs;
    use raiko_reqpool::memory_pool;
    use std::time::SystemTime;
    use tokio::sync::Mutex;

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use alloy_primitives::Address;
    use raiko_core::interfaces::ProverSpecificOpts;
    use raiko_lib::{input::BlobProofType, primitives::B256, proof_type::ProofType, prover::Proof};
    use raiko_reqpool::{
        AggregationRequestEntity, AggregationRequestKey, BatchGuestInputRequestEntity,
        BatchGuestInputRequestKey, BatchProofRequestEntity, BatchProofRequestKey,
        RequestEntity, RequestKey,
    };

    // Test constants
    const TEST_CHAIN_ID: u64 = 1;
    const BASE_L1_BLOCK: u64 = 1_000_000;

    fn create_batch_guest_input_request_key(batch_id: u64) -> RequestKey {
        let key = BatchGuestInputRequestKey::new(
            TEST_CHAIN_ID,
            batch_id,
            BASE_L1_BLOCK + batch_id, // l1_inclusion_block_number
        );
        RequestKey::BatchGuestInput(key)
    }

    fn create_batch_guest_input_request_entity(batch_id: u64) -> RequestEntity {
        let entity = BatchGuestInputRequestEntity::new(
            batch_id,
            BASE_L1_BLOCK + batch_id,
            "ethereum".to_string(),
            "ethereum".to_string(),
            B256::from([0u8; 32]),
            BlobProofType::ProofOfEquivalence,
        );
        RequestEntity::BatchGuestInput(entity)
    }


    fn create_aggregation_request_key(agg_id: u64) -> RequestKey {
        let key = AggregationRequestKey::new(ProofType::Native, vec![agg_id]);
        RequestKey::Aggregation(key)
    }

    fn create_aggregation_request_entity(agg_id: u64) -> RequestEntity {
        let entity = AggregationRequestEntity::new(
            vec![agg_id],
            vec![Proof::default()],
            ProofType::Native,
            ProverSpecificOpts::default(),
        );
        RequestEntity::Aggregation(entity)
    }

    /// REAL PRODUCTION SCENARIO:
    /// - Client submits LOW priority requests (BatchGuestInput) continuously
    /// - When LOW completes → client submits MEDIUM priority (BatchProof)
    /// - Every 5 MEDIUM complete → client submits 1 HIGH priority (Aggregation)
    /// - Client only marks work complete when aggregation succeeds
    #[tokio::test]
    async fn test_priority_starvation_detection() {
        println!("TESTING: Realistic Client Workflow Dependency Chain");
        let max_queue_size = 1000;
        let max_concurrency = 10;
        let queue = Arc::new(Mutex::new(Queue::new(max_queue_size)));
        let notifier = Arc::new(Notify::new());

        let completed_low = Arc::new(Mutex::new(Vec::new()));
        let completed_medium = Arc::new(Mutex::new(Vec::new()));
        let completed_aggregations = Arc::new(AtomicUsize::new(0));

        let (completion_tx, mut completion_rx) = tokio::sync::mpsc::channel::<(String, u64)>(1000);
        let backend_handle = tokio::spawn({
            let queue = queue.clone();
            let notifier = notifier.clone();
            let completion_tx = completion_tx.clone();

            async move {
                let (done_tx, mut done_rx) = tokio::sync::mpsc::channel(1000);
                let semaphore = Arc::new(Semaphore::new(max_concurrency));

                loop {
                    while let Ok(request_key) = done_rx.try_recv() {
                        let mut queue = queue.lock().await;
                        queue.complete(request_key);
                    }

                    let permit = match semaphore.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => break,
                    };

                    let (request_key, request_entity) = {
                        let mut queue = queue.lock().await;
                        if let Some((request_key, request_entity)) = queue.try_next() {
                            (request_key, request_entity)
                        } else {
                            drop(queue);
                            drop(permit);
                            notifier.notified().await;
                            continue;
                        }
                    };

                    let request_key_ = request_key.clone();
                    let completion_tx_ = completion_tx.clone();
                    let done_tx_ = done_tx.clone();

                    tokio::spawn(async move {
                        let _permit = permit;

                        let (delay_ms, priority_label, id) = match &request_entity {
                            RequestEntity::Aggregation(e) => {
                                let agg_id = e.aggregation_ids()[0];
                                (1000, "HIGH", agg_id)
                            },
                            RequestEntity::BatchProof(e) => {
                                let batch_id = *e.guest_input_entity().batch_id();
                                (4000, "MEDIUM", batch_id)
                            },
                            RequestEntity::BatchGuestInput(e) => {
                                let batch_id = *e.batch_id();
                                (2000, "LOW", batch_id)
                            },
                            _ => (2000, "LOW", 0),
                        };

                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;

                        let _ = completion_tx_.send((priority_label.to_string(), id)).await;
                        let _ = done_tx_.send(request_key_).await;
                    });
                }
            }
        });

        let client_simulator = tokio::spawn({
            let queue = queue.clone();
            let notifier = notifier.clone();
            let completed_low = completed_low.clone();
            let completed_medium = completed_medium.clone();
            let completed_aggregations = completed_aggregations.clone();

            async move {
                let mut submitted_mediums = std::collections::HashSet::new();
                let mut submitted_aggregations = std::collections::HashSet::new();

                println!("CLIENT: Starting continuous LOW-priority request flood (500 requests)");
                {
                    let mut queue_guard = queue.lock().await;
                    for i in 0..500 {
                        let request_key = create_batch_guest_input_request_key(i);
                        let request_entity = create_batch_guest_input_request_entity(i);
                        let _ = queue_guard.add_pending(request_key, request_entity);
                    }
                    drop(queue_guard);
                    notifier.notify_one();
                }

                tokio::time::sleep(Duration::from_millis(500)).await;

                let workflow_start = SystemTime::now();
                let target_aggregations = 10;

                loop {
                    tokio::time::sleep(Duration::from_millis(50)).await;

                    let completed_low_list = completed_low.lock().await.clone();
                    for &low_id in &completed_low_list {
                        if !submitted_mediums.contains(&low_id) {
                            let mut queue_guard = queue.lock().await;
                            let batch_proof_key = BatchProofRequestKey::new(
                                TEST_CHAIN_ID,
                                low_id,
                                BASE_L1_BLOCK + low_id,
                                ProofType::Native,
                                "test_prover".to_string(),
                            );
                            let guest_input_entity = BatchGuestInputRequestEntity::new(
                                low_id,
                                BASE_L1_BLOCK + low_id,
                                "ethereum".to_string(),
                                "ethereum".to_string(),
                                B256::from([0u8; 32]),
                                BlobProofType::ProofOfEquivalence,
                            );
                            let batch_proof_entity = BatchProofRequestEntity::new_with_guest_input_entity(
                                guest_input_entity,
                                Address::ZERO,
                                ProofType::Native,
                                std::collections::HashMap::new(),
                            );
                            let _ = queue_guard.add_pending(
                                RequestKey::BatchProof(batch_proof_key),
                                RequestEntity::BatchProof(batch_proof_entity),
                            );
                            submitted_mediums.insert(low_id);
                            drop(queue_guard);
                            notifier.notify_one();
                            println!("CLIENT: Submitted MEDIUM request for completed LOW (id={})", low_id);
                        }
                    }

                    let completed_medium_list = completed_medium.lock().await.clone();
                    let num_aggregations_to_submit = (completed_medium_list.len() / 5).min(target_aggregations);

                    for agg_id in 1..=num_aggregations_to_submit {
                        if !submitted_aggregations.contains(&(agg_id as u64)) {
                            let mut queue_guard = queue.lock().await;
                            let request_key = create_aggregation_request_key(agg_id as u64);
                            let request_entity = create_aggregation_request_entity(agg_id as u64);
                            let _ = queue_guard.add_pending(request_key, request_entity);
                            submitted_aggregations.insert(agg_id as u64);
                            drop(queue_guard);
                            notifier.notify_one();
                            println!("CLIENT: Submitted HIGH-priority aggregation #{} (after {} MEDIUM completions)", agg_id, completed_medium_list.len());
                        }
                    }

                    let agg_count = completed_aggregations.load(Ordering::SeqCst);
                    if agg_count >= target_aggregations {
                        println!("CLIENT: Workflow complete! {} aggregations finished", agg_count);
                        break;
                    }

                    let elapsed = workflow_start.elapsed().unwrap().as_secs();
                    if elapsed > 60 {
                        panic!("DEADLOCK: Client workflow stuck - priority queue bug detected!");
                    }
                }
            }
        });

        println!("MONITORING: Tracking completion order and client workflow progression...");

        let mut completion_order = Vec::new();
        let mut aggregation_times = Vec::new();
        let start_time = SystemTime::now();
        let mut first_aggregation_time = None;

        while completion_order.len() < 200 && start_time.elapsed().unwrap().as_secs() < 120 {
            match tokio::time::timeout(Duration::from_millis(200), completion_rx.recv()).await {
                Ok(Some((priority, id))) => {
                    let elapsed = start_time.elapsed().unwrap().as_millis();
                    completion_order.push((priority.clone(), id, elapsed));

                    match priority.as_str() {
                        "LOW" => {
                            let mut low_list = completed_low.lock().await;
                            low_list.push(id);
                        }
                        "MEDIUM" => {
                            let mut medium_list = completed_medium.lock().await;
                            medium_list.push(id);
                        }
                        "HIGH" => {
                            let agg_count = completed_aggregations.fetch_add(1, Ordering::SeqCst) + 1;
                            aggregation_times.push(elapsed);
                            if first_aggregation_time.is_none() {
                                first_aggregation_time = Some(elapsed);
                            }
                            println!("AGGREGATION completed (id={}): {} aggregations done at {}ms", id, agg_count, elapsed);
                        }
                        _ => {}
                    }

                    if completed_aggregations.load(Ordering::SeqCst) >= 10 {
                        println!("ALL 10 AGGREGATIONS COMPLETED - Client workflow successful!");
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => continue,
            }
        }

        backend_handle.abort();
        client_simulator.abort();

        let aggregation_count = completed_aggregations.load(Ordering::SeqCst);
        let low_count = completed_low.lock().await.len();
        let medium_count = completed_medium.lock().await.len();

        println!("REAL PRODUCTION WORKFLOW ANALYSIS:");
        println!("   Aggregations completed: {}", aggregation_count);
        println!("   Medium requests completed: {}", medium_count);
        println!("   Low requests completed: {}", low_count);
        println!("   Total completions tracked: {}", completion_order.len());

        println!("CRITICAL PRODUCTION METRIC - DEPENDENCY CHAIN VALIDATION:");

        if let Some(first_agg_ms) = first_aggregation_time {
            println!("First aggregation completed at: {}ms", first_agg_ms);

            if aggregation_count >= 10 {
                let last_agg_ms = aggregation_times.last().unwrap();
                println!("Target aggregations ({}) completed at: {}ms", aggregation_count, last_agg_ms);
                println!("Time from start to completion: {}ms", last_agg_ms);

                if *last_agg_ms < 45000 {
                    println!("Dependency Chain Working!");
                } else {
                    panic!("CRITICAL FAILURE - DEPENDENCY CHAIN BROKEN!");
                }

            } else if aggregation_count >= 1 {
                println!("PARTIAL SUCCESS - Only {} / 10 aggregations completed", aggregation_count);
                panic!("PARTIAL STARVATION: Only {} / 10 aggregations - dependency chain incomplete!", aggregation_count);

            } else {
                println!("CRITICAL FAILURE - NO aggregations completed!");
                panic!("COMPLETE AGGREGATION STARVATION - production bug detected!");
            }

        } else {
            panic!("AGGREGATION STARVATION: No aggregations completed!");
        }

        println!("COMPLETION TIMELINE (first 30):");
        for (i, (priority, id, ms)) in completion_order.iter().take(30).enumerate() {
            println!("   {:2}. {:6} (id={:3}) at {:6}ms", i+1, priority, id, ms);
        }
    }
}
