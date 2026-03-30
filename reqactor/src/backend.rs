use std::sync::Arc;

use alloy_primitives::{Uint, B256};

use raiko_core::{
    interfaces::{aggregate_proofs, aggregate_shasta_proposals, ProofRequest},
    preflight::{
        parse_l1_batch_proposal_tx_for_pacaya_fork, parse_l1_batch_proposal_tx_for_shasta_fork,
    },
    provider::rpc::RpcBlockDataProvider,
    Raiko,
};
use raiko_lib::{
    consts::SupportedChainSpecs,
    input::{
        realtime::{RealTimeEventData, RealTimeProposal},
        AggregationGuestInput, AggregationGuestOutput, BlockProposedFork, GuestBatchInput,
        GuestInput, ShastaAggregationGuestInput,
    },
    prover::{IdWrite, Proof},
    utils::shasta_guest_input::{
        decode_guest_input_from_prover_arg_value, encode_guest_input_to_compress_b64_str,
        PROVER_ARG_SHASTA_GUEST_INPUT,
    },
};
use raiko_reqpool::{
    AggregationRequestEntity, BatchGuestInputRequestEntity, BatchProofRequestEntity,
    GuestInputRequestEntity, RealTimeInputRequestEntity, RealTimeProofRequestEntity, RequestEntity,
    RequestKey, ShastaInputRequestEntity, ShastaProofRequestEntity, SingleProofRequestEntity,
    Status, StatusWithContext,
};
use tokio::sync::{mpsc, Mutex, Notify};
use tracing::{debug, trace};

use crate::gpu_semaphore::GpuSemaphore;
use crate::queue::Queue;
use crate::Pool;

/// Backend runs in the background, and handles the actions from the actor.
#[derive(Clone, Debug)]
pub(crate) struct Backend {
    pool: Pool,
    chain_specs: SupportedChainSpecs,
    queue: Arc<Mutex<Queue>>,
    notifier: Arc<Notify>,
    gpu_semaphore: Arc<GpuSemaphore>,
    mock_key: Option<String>,
}

impl Backend {
    pub fn new(
        pool: Pool,
        chain_specs: SupportedChainSpecs,
        max_proving_concurrency: usize,
        queue: Arc<Mutex<Queue>>,
        notifier: Arc<Notify>,
        mock_key: Option<String>,
    ) -> Self {
        Self {
            pool,
            chain_specs,
            queue,
            notifier,
            gpu_semaphore: Arc::new(GpuSemaphore::new(max_proving_concurrency)),
            mock_key,
        }
    }

    /// Drain all completed requests from the channel (non-blocking).
    async fn drain_completions(&self, done_rx: &mut mpsc::Receiver<RequestKey>) {
        while let Ok(request_key) = done_rx.try_recv() {
            self.complete_request(request_key).await;
        }
    }

    /// Mark a single request as complete in the queue.
    async fn complete_request(&self, request_key: RequestKey) {
        let mut queue = self.queue.lock().await;
        queue.complete(request_key);
    }

    pub async fn serve_in_background(self) {
        let (done_tx, mut done_rx) = mpsc::channel(1000);

        loop {
            // Drain all completions first (non-blocking)
            self.drain_completions(&mut done_rx).await;

            // Check if there are requests in the queue
            let has_requests = {
                let queue = self.queue.lock().await;
                !queue.is_empty()
            };

            if !has_requests {
                // Nothing to do — wait for new work or a completion
                tokio::select! {
                    _ = self.notifier.notified() => continue,
                    Some(key) = done_rx.recv() => {
                        self.complete_request(key).await;
                        continue;
                    }
                }
            }

            // There are requests — wait for a GPU slot while staying responsive
            let gpu_permit = loop {
                tokio::select! {
                    permit = self.gpu_semaphore.acquire() => break permit,
                    Some(key) = done_rx.recv() => self.complete_request(key).await,
                }
            };

            // Pull highest-priority request with GPU slot in hand
            let next = {
                let mut queue = self.queue.lock().await;
                queue.try_next()
            };
            let (request_key, request_entity) = match next {
                Some(pair) => pair,
                None => {
                    drop(gpu_permit);
                    continue;
                }
            };

            let request_key_ = request_key.clone();
            let mut pool_ = self.pool.clone();
            let chain_specs = self.chain_specs.clone();
            let mock_key = self.mock_key.clone();

            // Clones for the watcher task (must be created before request_key is moved)
            let request_key_for_watcher = request_key;
            let done_tx_ = done_tx.clone();
            let notifier_ = self.notifier.clone();
            let mut pool_for_panic = self.pool.clone();

            let _ = pool_.update_status(
                request_key_for_watcher.clone(),
                StatusWithContext::new(Status::WorkInProgress, chrono::Utc::now()),
            );

            let handle = tokio::spawn(async move {
                let result = match request_entity {
                    RequestEntity::SingleProof(entity) => {
                        do_prove_single(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                            Some(gpu_permit.gpu_number()),
                        )
                        .await
                    }
                    RequestEntity::Aggregation(entity) => {
                        do_prove_aggregation(
                            &mut pool_,
                            request_key_.clone(),
                            entity,
                            Some(gpu_permit.gpu_number()),
                            mock_key.clone(),
                        )
                        .await
                    }
                    RequestEntity::BatchProof(entity) => {
                        do_prove_batch(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                            Some(gpu_permit.gpu_number()),
                            mock_key.clone(),
                        )
                        .await
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
                    RequestEntity::ShastaGuestInput(entity) => {
                        do_generate_shasta_proposal_guest_input(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                        )
                        .await
                    }
                    RequestEntity::ShastaProof(entity) => {
                        do_prove_shasta_proposal(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                            Some(gpu_permit.gpu_number()),
                            mock_key.clone(),
                        )
                        .await
                    }
                    RequestEntity::ShastaAggregation(entity) => {
                        do_shasta_aggregation(
                            &mut pool_,
                            request_key_.clone(),
                            entity,
                            Some(gpu_permit.gpu_number()),
                            mock_key.clone(),
                        )
                        .await
                    }
                    RequestEntity::RealTimeGuestInput(entity) => {
                        do_generate_realtime_guest_input(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                        )
                        .await
                    }
                    RequestEntity::RealTimeProof(entity) => {
                        do_prove_realtime(
                            &mut pool_,
                            &chain_specs,
                            request_key_.clone(),
                            entity,
                            Some(gpu_permit.gpu_number()),
                            mock_key.clone(),
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

                // GPU permit is automatically dropped here, releasing the semaphore
                drop(gpu_permit);
            });

            // Spawn a watcher task that handles both success and panic
            tokio::spawn(async move {
                match handle.await {
                    Ok(()) => {
                        let _ = done_tx_.send(request_key_for_watcher).await;
                    }
                    Err(join_err) => {
                        // Task panicked — mark as failed and release the queue slot
                        tracing::error!(
                            "Proving task panicked for {request_key_for_watcher}: {join_err}"
                        );
                        let _ = pool_for_panic.update_status(
                            request_key_for_watcher.clone(),
                            StatusWithContext::new(
                                Status::Failed {
                                    error: format!("proving task panicked: {join_err}"),
                                },
                                chrono::Utc::now(),
                            ),
                        );
                        let _ = done_tx_.send(request_key_for_watcher).await;
                    }
                }
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
        checkpoint: Default::default(),
        cached_event_data: None,
        gpu_number: None,
        last_anchor_block_number: None,
    };
    let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec.clone(), proof_request);
    let provider = RpcBlockDataProvider::new(&taiko_chain_spec.rpc.clone())
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
    gpu_number: Option<u32>,
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
        checkpoint: Default::default(),
        cached_event_data: None,
        gpu_number,
        last_anchor_block_number: None,
    };
    let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec.clone(), proof_request);
    let provider = RpcBlockDataProvider::new(&taiko_chain_spec.rpc.clone())
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
                && prover_data.actual_prover.eq(request_entity.prover()))
            {
                input.taiko.prover_data = raiko_lib::input::TaikoProverData {
                    graffiti: request_entity.graffiti().clone(),
                    actual_prover: request_entity.prover().clone(),
                    ..Default::default()
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
    gpu_number: Option<u32>,
    mock_key: Option<String>,
) -> Result<Proof, String> {
    let proof_type = request_key.proof_type().clone();
    let proofs = request_entity.proofs().clone();

    let input = AggregationGuestInput { proofs };
    let output = AggregationGuestOutput::default();
    let mut config = serde_json::to_value(request_entity.prover_args())
        .map_err(|err| format!("failed to serialize prover args: {err:?}"))?;

    if let Some(gpu_number) = gpu_number {
        // If gpu_number is provided, we set it in the config
        config["gpu_number"] = gpu_number.into();
    }

    let proof = aggregate_proofs(proof_type, input, &output, &config, Some(pool), mock_key)
        .await
        .map_err(|err| format!("failed to generate aggregation proof: {err:?}"))?;

    Ok(proof)
}

async fn do_shasta_aggregation(
    pool: &mut dyn IdWrite,
    request_key: RequestKey,
    request_entity: AggregationRequestEntity,
    gpu_number: Option<u32>,
    mock_key: Option<String>,
) -> Result<Proof, String> {
    let proof_type = request_key.proof_type().clone();
    let proofs = request_entity.proofs().clone();

    let input = ShastaAggregationGuestInput { proofs };
    let output = AggregationGuestOutput { hash: B256::ZERO };
    let mut config = serde_json::to_value(request_entity.prover_args())
        .map_err(|err| format!("failed to serialize prover args: {err:?}"))?;

    if let Some(gpu_number) = gpu_number {
        // If gpu_number is provided, we set it in the config
        config["gpu_number"] = gpu_number.into();
    }

    let proof =
        aggregate_shasta_proposals(proof_type, input, &output, &config, Some(pool), mock_key)
            .await
            .map_err(|err| format!("failed to generate aggregation proof: {err:?}"))?;

    Ok(proof)
}

async fn new_raiko_for_batch_request(
    chain_specs: &SupportedChainSpecs,
    request_entity: BatchProofRequestEntity,
    gpu_number: Option<u32>,
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
    let (all_prove_blocks, cached_event_data) = parse_l1_batch_proposal_tx_for_pacaya_fork(
        &l1_chain_spec,
        &taiko_chain_spec,
        *l1_include_block_number,
        *batch_id,
    )
    .await
    .map_err(|err| format!("Could not parse pacaya L1 batch proposal tx: {err:?}"))?;

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
        checkpoint: Default::default(),
        cached_event_data: Some(cached_event_data),
        gpu_number,
        last_anchor_block_number: None,
    };

    Ok(Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request))
}

async fn generate_input_for_batch(raiko: &Raiko) -> Result<GuestBatchInput, String> {
    let provider = RpcBlockDataProvider::new(&raiko.taiko_chain_spec.rpc)
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
    let raiko = new_raiko_for_batch_request(chain_specs, batch_proof_request_entity, None)
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

async fn do_prove_batch(
    pool: &mut dyn IdWrite,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: BatchProofRequestEntity,
    gpu_number: Option<u32>,
    mock_key: Option<String>,
) -> Result<Proof, String> {
    tracing::info!("Generating proof for {request_key}");
    let raiko = new_raiko_for_batch_request(chain_specs, request_entity, gpu_number).await?;
    let input = if let Some(batch_guest_input) = raiko.request.prover_args.get("batch_guest_input")
    {
        // Tricky: originally the input was created (and pass around) by prove() infra,
        // so it's a base64 string(in Proof).
        // after we get it from db somewhere before, we need to pass it down here, but there is no known
        // string carrier in key / entity, so we call deser twice, value -> string -> struct.
        decode_guest_input_from_prover_arg_value(batch_guest_input)?
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
        .batch_prove(input, &output, Some(pool), mock_key)
        .await
        .map_err(|e| format!("failed to generate batch proof: {e:?}"))?;

    Ok(proof)
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
    let raiko =
        new_raiko_for_shasta_proposal_request(chain_specs, shasta_proposal_request_entity, None)
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
    gpu_number: Option<u32>,
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
        gpu_number,
    };

    Ok(Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request))
}

pub async fn do_prove_shasta_proposal(
    _pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: ShastaProofRequestEntity,
    gpu_number: Option<u32>,
    mock_key: Option<String>,
) -> Result<Proof, String> {
    tracing::info!("generate shasta proposal proof for: {request_key:?}");

    let raiko =
        new_raiko_for_shasta_proposal_request(chain_specs, request_entity.clone(), gpu_number)
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
        .shasta_proposal_prove(input, &output, None, mock_key)
        .await
        .map_err(|err| format!("failed to run shasta proposal prover: {err:?}"))?;

    Ok(proof)
}

// === RealTime fork backend functions ===

async fn new_raiko_for_realtime_request(
    chain_specs: &SupportedChainSpecs,
    request_entity: RealTimeProofRequestEntity,
    gpu_number: Option<u32>,
) -> Result<Raiko, String> {
    let l1_chain_spec = chain_specs
        .get_chain_spec(&request_entity.guest_input_entity().l1_network())
        .expect("unsupported l1 network");
    let taiko_chain_spec = chain_specs
        .get_chain_spec(&request_entity.guest_input_entity().network())
        .expect("unsupported taiko network");

    let entity = request_entity.guest_input_entity();

    // Build RealTimeEventData from the stored entity fields.
    // maxAnchorBlockHash and signalSlotsHash are left as zero — they are filled in by
    // prepare_taiko_chain_batch_input_realtime during guest-input generation.
    let realtime_event_data = RealTimeEventData {
        proposal: RealTimeProposal {
            maxAnchorBlockNumber: Uint::from(*entity.max_anchor_block_number()),
            maxAnchorBlockHash: B256::ZERO,
            basefeeSharingPctg: *entity.basefee_sharing_pctg(),
            sources: entity.sources().clone(),
            signalSlotsHash: B256::ZERO,
        },
        signal_slots: entity.signal_slots().clone(),
        last_finalized_block_hash: *entity.last_finalized_block_hash(),
        blobs: entity.blobs().clone(),
    };

    let proof_request = ProofRequest {
        block_number: 0,
        batch_id: 0, // RealTime has no on-chain proposal ID
        l1_inclusion_block_number: 0,
        network: entity.network().clone(),
        l1_network: entity.l1_network().clone(),
        graffiti: Default::default(),
        prover: *entity.actual_prover(),
        proof_type: *request_entity.proof_type(),
        blob_proof_type: entity.blob_proof_type().clone(),
        prover_args: request_entity.prover_args().clone(),
        l2_block_numbers: entity.l2_block_numbers().clone(),
        checkpoint: entity.checkpoint().clone(),
        last_anchor_block_number: Some(*entity.max_anchor_block_number()),
        cached_event_data: Some(BlockProposedFork::RealTime(realtime_event_data)),
        gpu_number,
    };

    Ok(Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request))
}

pub async fn do_generate_realtime_guest_input(
    _pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: RealTimeInputRequestEntity,
) -> Result<Proof, String> {
    trace!("generate realtime guest input for: {request_key:?}");
    let realtime_proof_request_entity = RealTimeProofRequestEntity::new_with_guest_input_entity(
        request_entity.clone(),
        Default::default(),
        Default::default(),
    );
    let raiko = new_raiko_for_realtime_request(chain_specs, realtime_proof_request_entity, None)
        .await
        .map_err(|err| format!("failed to create raiko: {err:?}"))?;
    let input = generate_input_for_batch(&raiko)
        .await
        .map_err(|err| format!("failed to generate realtime guest input: {err:?}"))?;
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

pub async fn do_prove_realtime(
    _pool: &mut Pool,
    chain_specs: &SupportedChainSpecs,
    request_key: RequestKey,
    request_entity: RealTimeProofRequestEntity,
    gpu_number: Option<u32>,
    mock_key: Option<String>,
) -> Result<Proof, String> {
    tracing::info!("generate realtime proof for: {request_key:?}");

    let raiko = new_raiko_for_realtime_request(chain_specs, request_entity.clone(), gpu_number)
        .await
        .map_err(|err| format!("failed to create raiko: {err:?}"))?;

    let input = if let Some(realtime_guest_input) =
        raiko.request.prover_args.get(PROVER_ARG_SHASTA_GUEST_INPUT)
    {
        decode_guest_input_from_prover_arg_value(realtime_guest_input)?
    } else {
        tracing::warn!("rebuild realtime guest input for request: {request_key:?}");
        generate_input_for_batch(&raiko)
            .await
            .map_err(|err| format!("failed to generate realtime guest input: {err:?}"))?
    };

    // Generate the output for the batch
    let output = raiko
        .get_batch_output(&input)
        .map_err(|err| format!("failed to generate output: {err:?}"))?;

    // Run the RealTime prover
    let proof = raiko
        .realtime_prove(input, &output, None, mock_key)
        .await
        .map_err(|err| format!("failed to run realtime prover: {err:?}"))?;

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

        let backend = Backend::new(pool, chain_specs, 1, queue.clone(), notifier.clone(), None);

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
