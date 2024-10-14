use std::{
    collections::{HashMap, VecDeque},
    str::FromStr,
    sync::Arc,
};

use anyhow::anyhow;
use raiko_core::{
    interfaces::{AggregationOnlyRequest, ProofRequest, ProofType, RaikoError},
    provider::{get_task_data, rpc::RpcBlockDataProvider},
    Raiko,
};
use raiko_lib::{
    consts::SupportedChainSpecs,
    input::{AggregationGuestInput, AggregationGuestOutput},
    prover::{IdWrite, Proof},
    Measurement,
};
use raiko_tasks::{get_task_manager, TaskDescriptor, TaskManager, TaskManagerWrapper, TaskStatus};
use reth_primitives::B256;
use tokio::{
    select,
    sync::{
        mpsc::{Receiver, Sender},
        Mutex, OwnedSemaphorePermit, Semaphore,
    },
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{
    cache,
    interfaces::{HostError, HostResult},
    memory,
    metrics::{
        inc_guest_error, inc_guest_success, inc_host_error, observe_guest_time,
        observe_prepare_input_time, observe_total_time,
    },
    Message, Opts,
};

pub struct ProofActor {
    opts: Opts,
    chain_specs: SupportedChainSpecs,
    aggregate_tasks: Arc<Mutex<HashMap<AggregationOnlyRequest, CancellationToken>>>,
    running_tasks: Arc<Mutex<HashMap<TaskDescriptor, CancellationToken>>>,
    pending_tasks: Arc<Mutex<VecDeque<ProofRequest>>>,
    receiver: Receiver<Message>,
    sender: Sender<Message>,
}

impl ProofActor {
    pub fn new(
        sender: Sender<Message>,
        receiver: Receiver<Message>,
        opts: Opts,
        chain_specs: SupportedChainSpecs,
    ) -> Self {
        let running_tasks = Arc::new(Mutex::new(
            HashMap::<TaskDescriptor, CancellationToken>::new(),
        ));
        let aggregate_tasks = Arc::new(Mutex::new(HashMap::<
            AggregationOnlyRequest,
            CancellationToken,
        >::new()));
        let pending_tasks = Arc::new(Mutex::new(VecDeque::<ProofRequest>::new()));

        Self {
            opts,
            chain_specs,
            aggregate_tasks,
            running_tasks,
            pending_tasks,
            receiver,
            sender,
        }
    }

    pub async fn cancel_task(&mut self, key: TaskDescriptor) -> HostResult<()> {
        let tasks_map = self.running_tasks.lock().await;
        let Some(task) = tasks_map.get(&key) else {
            warn!("No task with those keys to cancel");
            return Ok(());
        };

        let mut manager = get_task_manager(&self.opts.clone().into());
        key.proof_system
            .cancel_proof(
                (key.chain_id, key.blockhash, key.proof_system as u8),
                Box::new(&mut manager),
            )
            .await
            .or_else(|e| {
                if e.to_string().contains("No data for query") {
                    warn!("Task already cancelled or not yet started!");
                    Ok(())
                } else {
                    Err::<(), HostError>(e.into())
                }
            })?;
        task.cancel();
        Ok(())
    }

    pub async fn run_task(&mut self, proof_request: ProofRequest) {
        let cancel_token = CancellationToken::new();

        let Ok((chain_id, blockhash)) = get_task_data(
            &proof_request.network,
            proof_request.block_number,
            &self.chain_specs,
        )
        .await
        else {
            error!("Could not get task data for {proof_request:?}");
            return;
        };

        let key = TaskDescriptor::from((
            chain_id,
            blockhash,
            proof_request.proof_type,
            proof_request.prover.clone().to_string(),
        ));

        let mut tasks = self.running_tasks.lock().await;
        tasks.insert(key.clone(), cancel_token.clone());
        let sender = self.sender.clone();

        let tasks = self.running_tasks.clone();
        let opts = self.opts.clone();
        let chain_specs = self.chain_specs.clone();

        tokio::spawn(async move {
            select! {
                _ = cancel_token.cancelled() => {
                    info!("Task cancelled");
                }
                result = Self::handle_message(proof_request.clone(), key.clone(), &opts, &chain_specs) => {
                    match result {
                        Ok(status) => {
                            info!("Host handling message: {status:?}");
                        }
                        Err(error) => {
                            error!("Worker failed due to: {error:?}");
                        }
                    };
                }
            }
            let mut tasks = tasks.lock().await;
            tasks.remove(&key);
            // notify complete task to let next pending task run
            sender
                .send(Message::TaskComplete(proof_request))
                .await
                .expect("Couldn't send message");
        });
    }

    pub async fn cancel_aggregation_task(
        &mut self,
        request: AggregationOnlyRequest,
    ) -> HostResult<()> {
        let tasks_map = self.aggregate_tasks.lock().await;
        let Some(task) = tasks_map.get(&request) else {
            warn!("No task with those keys to cancel");
            return Ok(());
        };

        // TODO:(petar) implement cancel_proof_aggregation
        // let mut manager = get_task_manager(&self.opts.clone().into());
        // let proof_type = ProofType::from_str(
        //     request
        //         .proof_type
        //         .as_ref()
        //         .ok_or_else(|| anyhow!("No proof type"))?,
        // )?;
        // proof_type
        //     .cancel_proof_aggregation(request, Box::new(&mut manager))
        //     .await
        //     .or_else(|e| {
        //         if e.to_string().contains("No data for query") {
        //             warn!("Task already cancelled or not yet started!");
        //             Ok(())
        //         } else {
        //             Err::<(), HostError>(e.into())
        //         }
        //     })?;
        task.cancel();
        Ok(())
    }

    pub async fn run_aggregate(
        &mut self,
        request: AggregationOnlyRequest,
        _permit: OwnedSemaphorePermit,
    ) {
        let cancel_token = CancellationToken::new();

        let mut tasks = self.aggregate_tasks.lock().await;
        tasks.insert(request.clone(), cancel_token.clone());

        let request_clone = request.clone();
        let tasks = self.aggregate_tasks.clone();
        let opts = self.opts.clone();

        tokio::spawn(async move {
            select! {
                _ = cancel_token.cancelled() => {
                    info!("Task cancelled");
                }
                result = Self::handle_aggregate(request_clone, &opts) => {
                    match result {
                        Ok(()) => {
                            info!("Host handling message");
                        }
                        Err(error) => {
                            error!("Worker failed due to: {error:?}");
                        }
                    };
                }
            }
            let mut tasks = tasks.lock().await;
            tasks.remove(&request);
        });
    }

    pub async fn run(&mut self) {
        // recv() is protected by outside mpsc, no lock needed here
        let semaphore = Arc::new(Semaphore::new(self.opts.concurrency_limit));
        while let Some(message) = self.receiver.recv().await {
            match message {
                Message::Cancel(key) => {
                    debug!("Message::Cancel task: {key:?}");
                    if let Err(error) = self.cancel_task(key).await {
                        error!("Failed to cancel task: {error}")
                    }
                }
                Message::Task(proof_request) => {
                    debug!("Message::Task proof_request: {proof_request:?}");
                    let running_task_count = self.running_tasks.lock().await.len();
                    if running_task_count < self.opts.concurrency_limit {
                        info!("Running task {proof_request:?}");
                        self.run_task(proof_request).await;
                    } else {
                        info!(
                            "Task concurrency limit reached, current running {running_task_count:?}, pending: {:?}",
                            self.pending_tasks.lock().await.len()
                        );
                        let mut pending_tasks = self.pending_tasks.lock().await;
                        pending_tasks.push_back(proof_request);
                    }
                }
                Message::TaskComplete(req) => {
                    // pop up pending task if any task complete
                    debug!("Message::TaskComplete: {req:?}");
                    info!(
                        "task completed, current running {:?}, pending: {:?}",
                        self.running_tasks.lock().await.len(),
                        self.pending_tasks.lock().await.len()
                    );
                    let mut pending_tasks = self.pending_tasks.lock().await;
                    if let Some(proof_request) = pending_tasks.pop_front() {
                        info!("Pop out pending task {proof_request:?}");
                        self.sender
                            .send(Message::Task(proof_request))
                            .await
                            .expect("Couldn't send message");
                    }
                }
                Message::CancelAggregate(request) => {
                    if let Err(error) = self.cancel_aggregation_task(request).await {
                        error!("Failed to cancel task: {error}")
                    }
                }
                Message::Aggregate(request) => {
                    let permit = Arc::clone(&semaphore)
                        .acquire_owned()
                        .await
                        .expect("Couldn't acquire permit");
                    self.run_aggregate(request, permit).await;
                }
            }
        }
    }

    pub async fn handle_message(
        proof_request: ProofRequest,
        key: TaskDescriptor,
        opts: &Opts,
        chain_specs: &SupportedChainSpecs,
    ) -> HostResult<TaskStatus> {
        let mut manager = get_task_manager(&opts.clone().into());

        let status = manager.get_task_proving_status(&key).await?;

        if let Some(latest_status) = status.iter().last() {
            if !matches!(latest_status.0, TaskStatus::Registered) {
                return Ok(latest_status.0.clone());
            }
        }

        manager
            .update_task_progress(key.clone(), TaskStatus::WorkInProgress, None)
            .await?;

        let (status, proof) =
            match handle_proof(&proof_request, opts, chain_specs, Some(&mut manager)).await {
                Err(error) => {
                    error!("{error}");
                    (error.into(), None)
                }
                Ok(proof) => (TaskStatus::Success, Some(serde_json::to_vec(&proof)?)),
            };

        manager
            .update_task_progress(key, status.clone(), proof.as_deref())
            .await
            .map_err(HostError::from)?;
        Ok(status)
    }

    pub async fn handle_aggregate(request: AggregationOnlyRequest, opts: &Opts) -> HostResult<()> {
        let mut manager = get_task_manager(&opts.clone().into());

        let status = manager
            .get_aggregation_task_proving_status(&request)
            .await?;

        if let Some(latest_status) = status.iter().last() {
            if !matches!(latest_status.0, TaskStatus::Registered) {
                return Ok(());
            }
        }

        manager
            .update_aggregation_task_progress(&request, TaskStatus::WorkInProgress, None)
            .await?;
        let proof_type = ProofType::from_str(
            request
                .proof_type
                .as_ref()
                .ok_or_else(|| anyhow!("No proof type"))?,
        )?;
        let input = AggregationGuestInput {
            proofs: request.clone().proofs,
        };
        let output = AggregationGuestOutput { hash: B256::ZERO };
        let config = serde_json::to_value(request.clone().prover_args)?;
        let mut manager = get_task_manager(&opts.clone().into());

        let (status, proof) = match proof_type
            .aggregate_proofs(input, &output, &config, Some(&mut manager))
            .await
        {
            Err(error) => {
                error!("{error}");
                (HostError::from(error).into(), None)
            }
            Ok(proof) => (TaskStatus::Success, Some(serde_json::to_vec(&proof)?)),
        };

        manager
            .update_aggregation_task_progress(&request, status, proof.as_deref())
            .await?;

        Ok(())
    }
}

pub async fn handle_proof(
    proof_request: &ProofRequest,
    opts: &Opts,
    chain_specs: &SupportedChainSpecs,
    store: Option<&mut TaskManagerWrapper>,
) -> HostResult<Proof> {
    info!(
        "Generating proof for block {} on {}",
        proof_request.block_number, proof_request.network
    );

    // Check for a cached input for the given request config.
    let cached_input = cache::get_input(
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
    let input = match cache::validate_input(cached_input, &provider).await {
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
    let proof = raiko
        .prove(input.clone(), &output, store.map(|s| s as &mut dyn IdWrite))
        .await
        .map_err(|e| {
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
    cache::set_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
        &input,
    )?;

    Ok(proof)
}
