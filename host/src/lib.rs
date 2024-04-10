// Required for SP1
#![feature(generic_const_exprs)]
#![allow(incomplete_features)]

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

pub mod error;
pub mod execution;
pub mod metrics;
pub mod preflight;
pub mod provider_db;
pub mod request;
pub mod server;

use std::{alloc, fmt::Debug, path::PathBuf};

use anyhow::{Context, Result};
use cap::Cap;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

use crate::{error::HostError, request::ProofRequestOpt};

#[global_allocator]
static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::MAX);

#[derive(StructOpt, Default, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Opt {
    #[structopt(long, require_equals = true, default_value = "0.0.0.0:8080")]
    /// Server bind address
    /// [default: 0.0.0.0:8080]
    address: String,

    #[structopt(long, require_equals = true, default_value = "16")]
    /// Limit the max number of in-flight requests
    pub concurrency_limit: usize,

    #[structopt(long, require_equals = true)]
    pub log_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, default_value = "7")]
    pub max_log: usize,

    #[structopt(long, require_equals = true, default_value = "host/config/config.json")]
    /// Path to a config file that includes sufficent json args to request
    /// a proof of specified type. Curl json-rpc overrides its contents
    config_path: PathBuf,

    #[structopt(long, require_equals = true)]
    /// Use a local directory as a cache for input. Accepts a custom directory.
    cache_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, env = "RUST_LOG", default_value = "info")]
    /// Set the log level
    pub log_level: String,

    #[structopt(flatten)]
    /// Proof request options
    pub proof_request_opt: ProofRequestOpt,
}

#[derive(Debug, Clone)]
pub struct ProverState {
    pub opts: Opt,
}

impl ProverState {
    pub fn init() -> Result<Self, HostError> {
        // Read the command line arguments;
        let mut opts = Opt::from_args();
        // Read the config file.
        let mut file_config = ProofRequestOpt::from_file(&opts.config_path)?;
        // Merge the config file with the command line arguments so that command line
        // arguments take precedence.
        file_config.merge(&opts.proof_request_opt);
        opts.proof_request_opt = file_config;

        // Check if the cache path exists and create it if it doesn't.
        if let Some(cache_path) = &opts.cache_path {
            if !cache_path.exists() {
                std::fs::create_dir_all(cache_path).context("Could not create cache dir")?;
            }
        }

        Ok(Self { opts })
    }
}

mod memory {
    use crate::ALLOCATOR;

    pub(crate) fn reset_stats() {
        ALLOCATOR.reset_stats();
    }

    pub(crate) fn get_max_allocated() -> usize {
        ALLOCATOR.max_allocated()
    }

    pub(crate) fn print_stats(title: &str) {
        let max_memory = get_max_allocated();
        println!(
            "{}{}.{:06} MB",
            title,
            max_memory / 1000000,
            max_memory % 1000000
        );
    }
}
