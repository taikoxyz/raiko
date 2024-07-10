use std::{collections::HashMap, sync::Arc};

use raiko_core::{
    interfaces::{ProofRequest, RaikoError},
    provider::{get_task_data, rpc::RpcBlockDataProvider},
    Raiko,
};
use raiko_lib::{consts::SupportedChainSpecs, Measurement};
use raiko_task_manager::{get_task_manager, TaskDescriptor, TaskManager, TaskStatus};
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
    Opts,
};

pub struct ProofActor {
    rx: Receiver<ProofRequest>,
    opts: Opts,
    chain_specs: SupportedChainSpecs,
    tasks: Arc<Mutex<HashMap<TaskDescriptor, CancellationToken>>>,
}

impl ProofActor {
    pub fn new(
        rx: Receiver<ProofRequest>,
        cancel_rx: Receiver<TaskDescriptor>,
        opts: Opts,
        chain_specs: SupportedChainSpecs,
    ) -> Self {
        let tasks = Arc::new(Mutex::new(
            HashMap::<TaskDescriptor, CancellationToken>::new(),
        ));

        Self::spawn_cancel_listener(cancel_rx, tasks.clone());

        Self {
            rx,
            tasks,
            opts,
            chain_specs,
        }
    }

    /// Spawn a task that listens to the cancel channel to send the cancel signal to the correct
    /// cancellation token.
    pub fn spawn_cancel_listener(
        cancel_rx: Receiver<TaskDescriptor>,
        tasks: Arc<Mutex<HashMap<TaskDescriptor, CancellationToken>>>,
    ) {
        tokio::spawn(async move {
            let mut cancel_rx = cancel_rx;

            while let Some(key) = cancel_rx.recv().await {
                let tasks_map = tasks.lock().await;
                let Some(task) = tasks_map.get(&key) else {
                    warn!("No task with those keys to cancel");
                    continue;
                };

                task.cancel();
            }
        });
    }

    pub async fn run(&mut self) {
        let semaphore = Arc::new(Semaphore::new(self.opts.concurrency_limit));

        while let Some(proof_request) = self.rx.recv().await {
            let permit = Arc::clone(&semaphore).acquire_owned().await;
            let cancel_token = CancellationToken::new();

            let Ok((chain_id, blockhash)) = get_task_data(
                &proof_request.network,
                proof_request.block_number,
                &self.chain_specs,
            )
            .await
            else {
                error!("Could not get task data for {proof_request:?}");
                continue;
            };

            let key = TaskDescriptor::from((
                chain_id,
                blockhash,
                proof_request.proof_type,
                proof_request.prover.clone().to_string(),
            ));

            let mut tasks = self.tasks.lock().await;
            tasks.insert(key.clone(), cancel_token.clone());

            let tasks = self.tasks.clone();
            let opts = self.opts.clone();
            let chain_specs = self.chain_specs.clone();

            tokio::spawn(async move {
                let _permit = permit;
                select! {
                    _ = cancel_token.cancelled() => {
                        info!("Task cancelled");
                    }
                    result = Self::handle_message(proof_request, key.clone(), &opts, &chain_specs) => {
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
        proof_request: ProofRequest,
        key: TaskDescriptor,
        opts: &Opts,
        chain_specs: &SupportedChainSpecs,
    ) -> HostResult<()> {
        let mut manager = get_task_manager(&opts.clone().into());

        let status = manager.get_task_proving_status(&key).await?;

        if let Some(latest_status) = status.iter().last() {
            if !matches!(latest_status.0, TaskStatus::Registered) {
                return Ok(());
            }
        }

        manager
            .update_task_progress(key.clone(), TaskStatus::WorkInProgress, None)
            .await?;

        match handle_proof(&proof_request, opts, chain_specs).await {
            Ok(ProofResponse { proof, .. }) => {
                let proof_string = proof.unwrap_or_default();
                let proof = proof_string.as_bytes();

                manager
                    .update_task_progress(key.clone(), TaskStatus::Success, Some(proof))
                    .await?;
                Ok(())
            }
            Err(error) => {
                manager
                    .update_task_progress(key, (&error).into(), None)
                    .await?;
                Err(error)
            }
        }
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
