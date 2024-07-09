use std::{collections::HashMap, sync::Arc};

use raiko_core::{
    interfaces::{ProofRequest, RaikoError},
    provider::{get_task_data, rpc::RpcBlockDataProvider},
    Raiko,
};
use raiko_lib::{consts::SupportedChainSpecs, Measurement};
use raiko_task_manager::{get_task_manager, TaskManager, TaskStatus};
use tokio::{
    select,
    sync::{mpsc::Receiver, Mutex, Semaphore},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{
    interfaces::{HostError, HostResult},
    memory,
    metrics::{
        inc_guest_error, inc_guest_success, inc_host_error, observe_guest_time,
        observe_prepare_input_time, observe_total_time,
    },
    server::api::v1::{
        proof::{get_cached_input, set_cached_input, validate_cache_input},
        ProofResponse,
    },
    CancelChannelOpts, Opts, TaskChannelOpts,
};

pub struct ProofActor {
    rx: Receiver<TaskChannelOpts>,
    tasks: Arc<Mutex<HashMap<CancelChannelOpts, CancellationToken>>>,
    task_count: usize,
}

impl ProofActor {
    pub fn new(
        rx: Receiver<TaskChannelOpts>,
        cancel_rx: Receiver<CancelChannelOpts>,
        task_count: usize,
    ) -> Self {
        let tasks = Arc::new(Mutex::new(
            HashMap::<CancelChannelOpts, CancellationToken>::new(),
        ));
        let cloned = tasks.clone();

        tokio::spawn(async move {
            let mut cancel_rx = cancel_rx;

            while let Some(key) = cancel_rx.recv().await {
                let tasks = cloned.lock().await;
                let Some(task) = tasks.get(&key) else {
                    warn!("No task with those keys to cancel");
                    continue;
                };

                task.cancel();
            }
        });

        Self {
            rx,
            tasks,
            task_count,
        }
    }

    pub async fn run(&mut self) {
        let semaphore = Arc::new(Semaphore::new(self.task_count));

        while let Some(message) = self.rx.recv().await {
            let permit = Arc::clone(&semaphore).acquire_owned().await;
            let cancel_token = CancellationToken::new();

            let (chain_id, blockhash) =
                get_task_data(&message.0.network, message.0.block_number, &message.2)
                    .await
                    .unwrap();

            let key = (
                chain_id,
                blockhash,
                message.0.proof_type,
                Some(message.0.prover.clone().to_string()),
            );

            let mut tasks = self.tasks.lock().await;
            tasks.insert(key.clone(), cancel_token.clone());

            let tasks = self.tasks.clone();

            tokio::spawn(async move {
                let _permit = permit;
                select! {
                    _ = cancel_token.cancelled() => {
                        info!("Task cancelled");
                    }
                    result = Self::handle_message(message) => {
                        match result {
                            Ok(()) => {
                                info!("Proof generated");
                            }
                            Err(error) => {
                                error!("Worker failed due to: {error:?}");
                            }
                        };
                    }
                }
                let mut tasks = tasks.lock().await;
                tasks.remove(&key);
            });
        }
    }

    pub async fn handle_message(
        (proof_request, opts, chain_specs): TaskChannelOpts,
    ) -> HostResult<()> {
        let (chain_id, blockhash) = get_task_data(
            &proof_request.network,
            proof_request.block_number,
            &chain_specs,
        )
        .await?;
        let mut manager = get_task_manager(&opts.clone().into());
        let status = manager
            .get_task_proving_status(
                chain_id,
                blockhash,
                proof_request.proof_type,
                Some(proof_request.prover.clone().to_string()),
            )
            .await?;

        if let Some(latest_status) = status.iter().last() {
            if !matches!(latest_status.0, TaskStatus::Registered) {
                return Ok(());
            }
        }

        manager
            .update_task_progress(
                chain_id,
                blockhash,
                proof_request.proof_type,
                Some(proof_request.prover.to_string()),
                TaskStatus::WorkInProgress,
                None,
            )
            .await?;

        match handle_proof(&proof_request, &opts, &chain_specs).await {
            Ok(result) => {
                let proof_string = result.proof.unwrap_or_default();
                let proof = proof_string.as_bytes();

                manager
                    .update_task_progress(
                        chain_id,
                        blockhash,
                        proof_request.proof_type,
                        Some(proof_request.prover.to_string()),
                        TaskStatus::Success,
                        Some(proof),
                    )
                    .await?;
            }
            Err(error) => {
                manager
                    .update_task_progress(
                        chain_id,
                        blockhash,
                        proof_request.proof_type,
                        Some(proof_request.prover.to_string()),
                        error.into(),
                        None,
                    )
                    .await?;
            }
        }

        Ok(())
    }
}

pub async fn handle_proof(
    proof_request: &ProofRequest,
    opts: &Opts,
    chain_specs: &SupportedChainSpecs,
) -> HostResult<ProofResponse> {
    info!(
        "# Generating proof for block {} on {}",
        proof_request.block_number, proof_request.network
    );

    // Check for a cached input for the given request config.
    let cached_input = get_cached_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
    );

    let l1_chain_spec = chain_specs
        .get_chain_spec(&proof_request.l1_network.to_string())
        .ok_or_else(|| HostError::InvalidRequestConfig("Unsupported l1 network".to_string()))?;

    let taiko_chain_spec = chain_specs
        .get_chain_spec(&proof_request.network.to_string())
        .ok_or_else(|| HostError::InvalidRequestConfig("Unsupported raiko network".to_string()))?;

    // Execute the proof generation.
    let total_time = Measurement::start("", false);

    let raiko = Raiko::new(
        l1_chain_spec.clone(),
        taiko_chain_spec.clone(),
        proof_request.clone(),
    );
    let provider = RpcBlockDataProvider::new(
        &taiko_chain_spec.rpc.clone(),
        proof_request.block_number - 1,
    )?;
    let input = match validate_cache_input(cached_input, &provider).await {
        Ok(cache_input) => cache_input,
        Err(_) => {
            // no valid cache
            memory::reset_stats();
            let measurement = Measurement::start("Generating input...", false);
            let input = raiko.generate_input(provider).await?;
            let input_time = measurement.stop_with("=> Input generated");
            observe_prepare_input_time(proof_request.block_number, input_time, true);
            memory::print_stats("Input generation peak memory used: ");
            input
        }
    };
    memory::reset_stats();
    let output = raiko.get_output(&input)?;
    memory::print_stats("Guest program peak memory used: ");

    memory::reset_stats();
    let measurement = Measurement::start("Generating proof...", false);
    let proof = raiko.prove(input.clone(), &output).await.map_err(|e| {
        let total_time = total_time.stop_with("====> Proof generation failed");
        observe_total_time(proof_request.block_number, total_time, false);
        match e {
            RaikoError::Guest(e) => {
                inc_guest_error(&proof_request.proof_type, proof_request.block_number);
                HostError::Core(e.into())
            }
            e => {
                inc_host_error(proof_request.block_number);
                e.into()
            }
        }
    })?;
    let guest_time = measurement.stop_with("=> Proof generated");
    observe_guest_time(
        &proof_request.proof_type,
        proof_request.block_number,
        guest_time,
        true,
    );
    memory::print_stats("Prover peak memory used: ");

    inc_guest_success(&proof_request.proof_type, proof_request.block_number);
    let total_time = total_time.stop_with("====> Complete proof generated");
    observe_total_time(proof_request.block_number, total_time, true);

    // Cache the input for future use.
    set_cached_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
        &input,
    )?;

    ProofResponse::try_from(proof)
}
