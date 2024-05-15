// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

pub mod interfaces;
pub mod metrics;
pub mod preflight;
pub mod provider;
pub mod raiko;
pub mod server;

use std::{alloc, collections::HashMap, path::PathBuf};

use crate::{error::HostResult, request::ProofRequestOpt};
use alloy_primitives::Address;
use alloy_rpc_types::EIP1186AccountProofResponse;
use anyhow::Context;
use cap::Cap;
use clap::Parser;
use raiko_lib::consts::SupportedChainSpecs;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::interfaces::{error::HostResult, request::ProofRequestOpt};
use tracing::info;

type MerkleProof = HashMap<Address, EIP1186AccountProofResponse>;

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

fn default_chain_spec_path() -> PathBuf {
    PathBuf::from("host/config/chain_spec_list_default.json")
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

    #[arg(
        long,
        require_equals = true,
        default_value = "host/config/chain_spec_list_default.json"
    )]
    #[serde(default = "default_chain_spec_path")]
    /// Path to a chain spec file that includes supported chain list
    chain_spec_path: PathBuf,

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

/// Merges two json's together, overwriting `a` with the values of `b`
fn merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) if !b.is_null() => *a = b.clone(),
        // If b is null, just keep a (which means do nothing).
        _ => {}
    }
}

#[derive(Debug, Clone)]
pub struct ProverState {
    pub opts: Cli,
    pub chain_specs: SupportedChainSpecs,
}

impl ProverState {
    pub fn init() -> HostResult<Self> {
        // Read the command line arguments;
        let mut opts = Cli::parse();
        // Read the config file.
        opts.merge_from_file()?;

        let chain_specs = SupportedChainSpecs::merge_from_file(opts.chain_spec_path.clone());
        info!("Supported chains: {:?}", chain_specs.supported_networks());

        // Check if the cache path exists and create it if it doesn't.
        if let Some(cache_path) = &opts.cache_path {
            if !cache_path.exists() {
                std::fs::create_dir_all(cache_path).context("Could not create cache dir")?;
            }
        }

        Ok(Self { opts, chain_specs })
    }
}

mod memory {
    use tracing::info;

    use crate::ALLOCATOR;

    pub(crate) fn reset_stats() {
        ALLOCATOR.reset_stats();
    }

    pub(crate) fn get_max_allocated() -> usize {
        ALLOCATOR.max_allocated()
    }

    pub(crate) fn print_stats(title: &str) {
        let max_memory = get_max_allocated();
        info!(
            "{title}{}.{:06} MB",
            max_memory / 1_000_000,
            max_memory % 1_000_000
        );
    }
}
