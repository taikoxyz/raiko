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

pub mod host;
mod metrics;
mod prover;

use std::{alloc, fmt::Debug, fs::File, io::BufReader, path::PathBuf};

use anyhow::Result;
use cap::Cap;
use prover::server::serve;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use structopt::StructOpt;

#[global_allocator]
static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::max_value());

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
            "{}{}.{} MB",
            title,
            max_memory / 1000000,
            max_memory % 1000000
        );
    }
}

#[derive(StructOpt, Default, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Opt {
    #[structopt(long, require_equals = true, default_value = "0.0.0.0:8080")]
    /// Server bind address
    /// [default: 0.0.0.0:8080]
    address: String,

    #[structopt(long, require_equals = true, default_value = "/tmp")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    cache: PathBuf,

    #[structopt(long, require_equals = true)]
    log_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, default_value = "1000")]
    proof_cache: usize,

    #[structopt(long, require_equals = true, default_value = "10")]
    concurrency_limit: usize,

    #[structopt(long, require_equals = true, default_value = "taiko_a7")]
    network: String,

    #[structopt(long, require_equals = true, default_value = "7")]
    max_log_days: usize,

    #[structopt(long, require_equals = true, default_value = "20")]
    // WARNING: must be larger than concurrency_limit
    max_caches: usize,

    #[structopt(long, require_equals = true)]
    config_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, env = "RUST_LOG", default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = get_config(None).unwrap();
    let opt = Opt::deserialize(&config).unwrap();
    println!("Start config: {:?}", opt);

    let subscriber_builder = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(&opt.log_level)
        .with_test_writer();
    let _guard = match opt.log_path {
        Some(ref log_path) => {
            let file_appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("raiko.log")
                .max_log_files(opt.max_log_days)
                .build(log_path)
                .expect("initializing rolling file appender failed");
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
            let subscriber = subscriber_builder.json().with_writer(non_blocking).finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            Some(_guard)
        }
        None => {
            let subscriber = subscriber_builder.finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            None
        }
    };

    serve(opt).await?;
    Ok(())
}

/// Gets the config going through all possible sources
pub fn get_config(request_config: Option<Value>) -> Result<Value> {
    let mut config = Value::default();
    let opt = Opt::from_args();

    // Config file has the lowest preference
    if let Some(config_path) = &opt.config_path {
        let file = File::open(config_path)?;
        let reader = BufReader::new(file);
        let file_config: Value = serde_json::from_reader(reader)?;
        merge(&mut config, &file_config);
    };

    // Command line values have higher preference
    let cli_config = serde_json::to_value(&opt)?;
    merge(&mut config, &cli_config);

    // Values sent via json-rpc have the highest preference
    if let Some(request_config) = request_config {
        merge(&mut config, &request_config);
    };

    Ok(config)
}

/// Merges two json's together, overwriting `a` with the values of `b`
fn merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b.clone(),
    }
}
