pub mod interfaces;
pub mod metrics;
pub mod server;

use std::{alloc, path::PathBuf};

use anyhow::Context;
use cap::Cap;
use clap::Parser;
use raiko_core::{
    interfaces::{ProofRequest, ProofRequestOpt, RaikoError},
    merge,
    provider::{get_task_data, rpc::RpcBlockDataProvider},
    Raiko,
};
use raiko_lib::{consts::SupportedChainSpecs, Measurement};
use raiko_task_manager::{get_task_manager, TaskManager, TaskManagerOpts, TaskStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::{
    interfaces::{HostError, HostResult},
    metrics::{
        inc_guest_error, inc_guest_req_count, inc_guest_success, inc_host_error,
        inc_host_req_count, observe_guest_time, observe_prepare_input_time, observe_total_time,
    },
    server::api::v1::{
        proof::{get_cached_input, set_cached_input, validate_cache_input},
        ProofResponse,
    },
};

#[global_allocator]
static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::MAX);

fn default_address() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_concurrency_limit() -> usize {
    16
}

fn default_max_log() -> usize {
    16
}

fn default_config_path() -> PathBuf {
    PathBuf::from("host/config/config.json")
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, Parser)]
#[command(
    name = "raiko",
    about = "The taiko prover host",
    long_about = None
)]
#[serde(default)]
pub struct Cli {
    #[arg(long, require_equals = true, default_value = "0.0.0.0:8080")]
    #[serde(default = "default_address")]
    /// Server bind address
    /// [default: 0.0.0.0:8080]
    address: String,

    #[arg(long, require_equals = true, default_value = "16")]
    #[serde(default = "default_concurrency_limit")]
    /// Limit the max number of in-flight requests
    pub concurrency_limit: usize,

    #[arg(long, require_equals = true)]
    pub log_path: Option<PathBuf>,

    #[arg(long, require_equals = true, default_value = "7")]
    #[serde(default = "default_max_log")]
    pub max_log: usize,

    #[arg(long, require_equals = true, default_value = "host/config/config.json")]
    #[serde(default = "default_config_path")]
    /// Path to a config file that includes sufficient json args to request
    /// a proof of specified type. Curl json-rpc overrides its contents
    config_path: PathBuf,

    #[arg(long, require_equals = true)]
    /// Path to a chain spec file that includes supported chain list
    chain_spec_path: Option<PathBuf>,

    #[arg(long, require_equals = true)]
    /// Use a local directory as a cache for input. Accepts a custom directory.
    cache_path: Option<PathBuf>,

    #[arg(long, require_equals = true, env = "RUST_LOG", default_value = "info")]
    #[serde(default = "default_log_level")]
    /// Set the log level
    pub log_level: String,

    #[command(flatten)]
    #[serde(flatten)]
    /// Proof request options
    pub proof_request_opt: ProofRequestOpt,

    #[arg(long, require_equals = true)]
    /// Set jwt secret for auth
    jwt_secret: Option<String>,

    #[arg(long, require_equals = true, default_value = "raiko.sqlite")]
    /// Set the path to the sqlite db file
    sqlite_file: PathBuf,

    #[arg(long, require_equals = true, default_value = "1048576")]
    max_db_size: usize,
}

impl Cli {
    /// Read the options from a file and merge it with the current options.
    pub fn merge_from_file(&mut self) -> HostResult<()> {
        let file = std::fs::File::open(&self.config_path)?;
        let reader = std::io::BufReader::new(file);
        let mut config: Value = serde_json::from_reader(reader)?;
        let this = serde_json::to_value(&self)?;
        merge(&mut config, &this);

        *self = serde_json::from_value(config)?;
        Ok(())
    }
}

type TaskChannelOpts = (ProofRequest, Cli, SupportedChainSpecs);

#[derive(Debug, Clone)]
pub struct ProverState {
    pub opts: Cli,
    pub chain_specs: SupportedChainSpecs,
    pub task_channel: mpsc::Sender<TaskChannelOpts>,
}

impl From<Cli> for TaskManagerOpts {
    fn from(val: Cli) -> Self {
        Self {
            sqlite_file: val.sqlite_file,
            max_db_size: val.max_db_size,
        }
    }
}

impl From<&Cli> for TaskManagerOpts {
    fn from(val: &Cli) -> Self {
        Self {
            sqlite_file: val.sqlite_file.clone(),
            max_db_size: val.max_db_size,
        }
    }
}

impl ProverState {
    pub fn init() -> HostResult<Self> {
        // Read the command line arguments;
        let mut opts = Cli::parse();
        // Read the config file.
        opts.merge_from_file()?;

        let chain_specs = if let Some(cs_path) = &opts.chain_spec_path {
            SupportedChainSpecs::merge_from_file(cs_path.clone()).unwrap_or_default()
        } else {
            SupportedChainSpecs::default()
        };

        // Check if the cache path exists and create it if it doesn't.
        if let Some(cache_path) = &opts.cache_path {
            if !cache_path.exists() {
                std::fs::create_dir_all(cache_path).context("Could not create cache dir")?;
            }
        }

        let (task_channel, mut receiver) = mpsc::channel::<TaskChannelOpts>(opts.concurrency_limit);

        let _spawn = tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                if let Err(error) = handle_message(message).await {
                    error!("Worker failed due to: {error:?}");
                }
            }
        });

        Ok(Self {
            opts,
            chain_specs,
            task_channel,
        })
    }
}

pub async fn handle_message(
    (proof_request, opts, chain_specs): (ProofRequest, Cli, SupportedChainSpecs),
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

pub async fn handle_proof(
    proof_request: &ProofRequest,
    opts: &Cli,
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

mod memory {
    use tracing::debug;

    use crate::ALLOCATOR;

    pub(crate) fn reset_stats() {
        ALLOCATOR.reset_stats();
    }

    pub(crate) fn get_max_allocated() -> usize {
        ALLOCATOR.max_allocated()
    }

    pub(crate) fn print_stats(title: &str) {
        let max_memory = get_max_allocated();
        debug!(
            "{title}{}.{:06} MB",
            max_memory / 1_000_000,
            max_memory % 1_000_000
        );
    }
}
