use std::{alloc, path::PathBuf};

use anyhow::Context;
use cap::Cap;
use clap::Parser;
use raiko_core::{
    interfaces::{ProofRequest, ProofRequestOpt},
    merge,
};
use raiko_lib::consts::SupportedChainSpecs;
use raiko_tasks::{get_task_manager, TaskDescriptor, TaskManagerOpts, TaskManagerWrapper};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{interfaces::HostResult, proof::ProofActor};

pub mod cache;
pub mod interfaces;
pub mod metrics;
pub mod proof;
pub mod server;

#[derive(Default, Clone, Serialize, Deserialize, Debug, Parser)]
#[command(
    name = "raiko",
    about = "The taiko prover host",
    long_about = None
)]
#[serde(default)]
pub struct Opts {
    #[arg(long, require_equals = true, default_value = "0.0.0.0:8080")]
    #[serde(default = "Opts::default_address")]
    /// Server bind address
    /// [default: 0.0.0.0:8080]
    address: String,

    #[arg(long, require_equals = true, default_value = "16")]
    #[serde(default = "Opts::default_concurrency_limit")]
    /// Limit the max number of in-flight requests
    pub concurrency_limit: usize,

    #[arg(long, require_equals = true)]
    pub log_path: Option<PathBuf>,

    #[arg(long, require_equals = true, default_value = "7")]
    #[serde(default = "Opts::default_max_log")]
    pub max_log: usize,

    #[arg(long, require_equals = true, default_value = "host/config/config.json")]
    #[serde(default = "Opts::default_config_path")]
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
    #[serde(default = "Opts::default_log_level")]
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

impl Opts {
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

impl From<Opts> for TaskManagerOpts {
    fn from(val: Opts) -> Self {
        Self {
            sqlite_file: val.sqlite_file,
            max_db_size: val.max_db_size,
        }
    }
}

impl From<&Opts> for TaskManagerOpts {
    fn from(val: &Opts) -> Self {
        Self {
            sqlite_file: val.sqlite_file.clone(),
            max_db_size: val.max_db_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProverState {
    pub opts: Opts,
    pub chain_specs: SupportedChainSpecs,
    pub task_channel: mpsc::Sender<Message>,
}

#[derive(Debug, Serialize)]
pub enum Message {
    Cancel(TaskDescriptor),
    Task(ProofRequest),
    TaskComplete(ProofRequest),
}

impl From<&ProofRequest> for Message {
    fn from(value: &ProofRequest) -> Self {
        Self::Task(value.clone())
    }
}

impl From<&TaskDescriptor> for Message {
    fn from(value: &TaskDescriptor) -> Self {
        Self::Cancel(value.clone())
    }
}

impl ProverState {
    pub fn init() -> HostResult<Self> {
        // Read the command line arguments;
        let mut opts = Opts::parse();
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

        let (task_channel, receiver) = mpsc::channel::<Message>(opts.concurrency_limit);

        let opts_clone = opts.clone();
        let chain_specs_clone = chain_specs.clone();
        let sender = task_channel.clone();
        tokio::spawn(async move {
            ProofActor::new(sender, receiver, opts_clone, chain_specs_clone)
                .run()
                .await;
        });

        Ok(Self {
            opts,
            chain_specs,
            task_channel,
        })
    }

    pub fn task_manager(&self) -> TaskManagerWrapper {
        get_task_manager(&(&self.opts).into())
    }

    pub fn request_config(&self) -> ProofRequestOpt {
        self.opts.proof_request_opt.clone()
    }
}

#[global_allocator]
static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::MAX);

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
        let mbs = max_memory / 1_000_000;
        let kbs = max_memory % 1_000_000;
        debug!("{title}{mbs}.{kbs:06} MB");
    }
}
