use raiko_core::{
    interfaces::{ProofRequest, RaikoError},
    provider::{get_task_data, rpc::RpcBlockDataProvider},
    Raiko,
};
use raiko_lib::{consts::SupportedChainSpecs, Measurement};
use raiko_task_manager::{get_task_manager, TaskManager, TaskStatus};
use tokio::sync::mpsc::Receiver;
use tracing::{error, info};

use crate::{
    interfaces::{HostError, HostResult},
    memory,
    metrics::{
        inc_guest_error, inc_guest_req_count, inc_guest_success, inc_host_error,
        inc_host_req_count, observe_guest_time, observe_prepare_input_time, observe_total_time,
    },
    server::api::v1::{
        proof::{get_cached_input, set_cached_input, validate_cache_input},
        ProofResponse,
    },
    Opts, TaskChannelOpts,
};

pub struct ProofActor {
    rx: Receiver<TaskChannelOpts>,
}

impl ProofActor {
    pub fn new(rx: Receiver<TaskChannelOpts>) -> Self {
        Self { rx }
    }

    pub async fn run(&mut self) {
        while let Some(message) = self.rx.recv().await {
            tokio::spawn(async move {
                if let Err(error) = Self::handle_message(message).await {
                    error!("Worker failed due to: {error:?}");
                }
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
        // If we cannot track progress with the task manager we can still do the work so we only trace
        // the error
        if manager
            .update_task_progress(
                chain_id,
                blockhash,
                proof_request.proof_type,
                Some(proof_request.prover.to_string()),
                TaskStatus::WorkInProgress,
                None,
            )
            .await
            .is_err()
        {
            error!("Could not update task to work in progress via task manager");
        }

        match handle_proof(&proof_request, &opts, &chain_specs).await {
            Ok(result) => {
                let proof_string = result.proof.unwrap_or_default();
                let proof = proof_string.as_bytes();
                // We don't need to fail here even if we cannot store it with task manager because the
                // work has already been done
                if manager
                    .update_task_progress(
                        chain_id,
                        blockhash,
                        proof_request.proof_type,
                        Some(proof_request.prover.to_string()),
                        TaskStatus::Success,
                        Some(proof),
                    )
                    .await
                    .is_err()
                {
                    error!("Could not update task progress to success via task manager");
                }
            }
            Err(error) => {
                // If we fail to track the with the task manager the work will be repeated anyway
                if manager
                    .update_task_progress(
                        chain_id,
                        blockhash,
                        proof_request.proof_type,
                        Some(proof_request.prover.to_string()),
                        error.into(),
                        None,
                    )
                    .await
                    .is_err()
                {
                    error!("Could not update task progress to error state via task manager");
                }
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
    inc_host_req_count(proof_request.block_number);
    inc_guest_req_count(&proof_request.proof_type, proof_request.block_number);

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
