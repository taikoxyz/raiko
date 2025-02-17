use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{alloc, path::PathBuf};

use anyhow::Context;
use cap::Cap;
use clap::Parser;
use raiko_ballot::Ballot;
use raiko_core::{
    interfaces::{AggregationOnlyRequest, ProofRequest, ProofRequestOpt},
    merge,
};
use raiko_lib::consts::SupportedChainSpecs;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::{get_task_manager, ProofTaskDescriptor, TaskManagerOpts, TaskManagerWrapperImpl};
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
    pub address: String,

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
    pub config_path: PathBuf,

    #[arg(long, require_equals = true)]
    /// Path to a chain spec file that includes supported chain list
    pub chain_spec_path: Option<PathBuf>,

    #[arg(long, require_equals = true)]
    /// Use a local directory as a cache for input. Accepts a custom directory.
    pub cache_path: Option<PathBuf>,

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
    pub jwt_secret: Option<String>,

    #[arg(long, require_equals = true, default_value = "1048576")]
    pub max_db_size: usize,

    #[arg(long, require_equals = true, default_value = "redis://localhost:6379")]
    pub redis_url: String,

    #[arg(long, require_equals = true, default_value = "3600")]
    pub redis_ttl: u64,

    #[arg(long, default_value = "false")]
    pub use_memory_backend: bool,

    /// Ballot config in json format. If not provided, '{}' will be used.
    #[arg(
        long,
        require_equals = true,
        default_value = "{}",
        help = "e.g. {\"Sp1\":0.1,\"Risc0\":0.2}"
    )]
    pub ballot: String,
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
        let file = std::fs::File::open(&self.config_path).context("Failed to open config file")?;
        let reader = std::io::BufReader::new(file);
        let mut config: Value =
            serde_json::from_reader(reader).context("Failed to read config file")?;
        let this = serde_json::to_value(&self).context("Failed to deserialize Opts")?;
        merge(&mut config, &this);

        *self = serde_json::from_value(config)?;
        Ok(())
    }

    pub fn merge_from_env(&mut self) {
        if let Some(path) = std::env::var("CONFIG_PATH").ok().map(PathBuf::from) {
            self.config_path = path;
        }
    }
}

impl From<Opts> for TaskManagerOpts {
    fn from(val: Opts) -> Self {
        Self {
            max_db_size: val.max_db_size,
            redis_url: val.redis_url.to_string(),
            redis_ttl: val.redis_ttl,
        }
    }
}

impl From<&Opts> for TaskManagerOpts {
    fn from(val: &Opts) -> Self {
        Self {
            max_db_size: val.max_db_size,
            redis_url: val.redis_url.to_string(),
            redis_ttl: val.redis_ttl,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProverState {
    pub opts: Opts,
    pub chain_specs: SupportedChainSpecs,
    pub task_channel: mpsc::Sender<Message>,
    pause_flag: Arc<AtomicBool>,
}

#[derive(Debug)]
pub enum Message {
    Cancel(ProofTaskDescriptor),
    Task(ProofRequest),
    TaskComplete(ProofRequest),
    CancelAggregate(AggregationOnlyRequest),
    Aggregate(AggregationOnlyRequest),
    SystemPause(tokio::sync::oneshot::Sender<HostResult<()>>),
}

impl ProverState {
    pub fn init() -> HostResult<Self> {
        let opts = parse_opts()?;
        Self::init_with_opts(opts)
    }

    pub fn init_with_opts(opts: Opts) -> HostResult<Self> {
        // Check if the cache path exists and create it if it doesn't.
        if let Some(cache_path) = &opts.cache_path {
            if !cache_path.exists() {
                std::fs::create_dir_all(cache_path).context("Could not create cache dir")?;
            }
        }

        let (task_channel, receiver) = mpsc::channel::<Message>(opts.concurrency_limit);
        let pause_flag = Arc::new(AtomicBool::new(false));

        let opts_clone = opts.clone();
        let chain_specs = parse_chain_specs(&opts);
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
            pause_flag,
        })
    }

    pub fn task_manager(&self) -> TaskManagerWrapperImpl {
        get_task_manager(&(&self.opts).into())
    }

    pub fn request_config(&self) -> ProofRequestOpt {
        self.opts.proof_request_opt.clone()
    }

    pub fn is_paused(&self) -> bool {
        self.pause_flag.load(Ordering::SeqCst)
    }

    /// Set the pause flag and notify the task manager to pause, then wait for the task manager to
    /// finish the pause process.
    ///
    /// Note that this function is blocking until the task manager finishes the pause process.
    pub async fn set_pause(&self, paused: bool) -> HostResult<()> {
        self.pause_flag.store(paused, Ordering::SeqCst);
        if paused {
            // Notify task manager to start pause process
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.task_channel
                .try_send(Message::SystemPause(sender))
                .context("Failed to send pause message")?;

            // Wait for the pause message to be processed
            let result = receiver.await.context("Failed to receive pause message")?;
            return result;
        }
        Ok(())
    }
}

pub fn parse_opts() -> HostResult<Opts> {
    // Read the command line arguments;
    let mut opts = Opts::parse();
    // Read env supported options.
    opts.merge_from_env();
    // Read the config file.
    opts.merge_from_file()?;

    Ok(opts)
}

pub fn parse_chain_specs(opts: &Opts) -> SupportedChainSpecs {
    if let Some(cs_path) = &opts.chain_spec_path {
        SupportedChainSpecs::merge_from_file(cs_path.clone()).expect("Failed to parse chain specs")
    } else {
        SupportedChainSpecs::default()
    }
}

pub fn parse_ballot(opts: &Opts) -> Ballot {
    let probs: BTreeMap<ProofType, f64> =
        serde_json::from_str(&opts.ballot).expect("Failed to parse ballot config");
    let ballot = Ballot::new(probs).expect("Failed to create ballot");
    ballot.validate().expect("Failed to validate ballot");
    ballot
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
