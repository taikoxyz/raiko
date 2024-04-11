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
pub mod preflight;
pub mod provider_db;
pub mod request;
pub mod server;

use std::{alloc, fmt::Debug, fs::File, io::BufReader, path::PathBuf};

use anyhow::Result;
use cap::Cap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use server::serve;
use structopt::StructOpt;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{Builder, Rotation},
};
use tracing_subscriber::FmtSubscriber;

#[global_allocator]
static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::max_value());

#[derive(StructOpt, Default, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Opt {
    #[structopt(long, require_equals = true, default_value = "0.0.0.0:8080")]
    /// Server bind address
    /// [default: 0.0.0.0:8080]
    address: String,

    #[structopt(long, require_equals = true, default_value = "16")]
    /// Limit the max number of in-flight requests
    concurrency_limit: usize,

    #[structopt(long, require_equals = true)]
    log_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, default_value = "7")]
    max_log: usize,

    #[structopt(long, require_equals = true, default_value = "host/config/config.json")]
    /// Path to a config file that includes sufficent json args to request
    /// a proof of specified type. Curl json-rpc overrides its contents
    config_path: PathBuf,

    #[structopt(long, require_equals = true)]
    /// Use a local directory as a cache for input. Accepts a custom directory.
    cache_path: Option<PathBuf>,

    #[structopt(long, require_equals = true, env = "RUST_LOG", default_value = "info")]
    /// Set the log level
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let config = get_config(None).unwrap();
    println!("Start config:\n{:#?}", config);
    println!("Args:\n{:#?}", opt);

    let _guard = subscribe_log(&opt.log_path, &opt.log_level, opt.max_log);

    serve(opt).await?;
    Ok(())
}

fn subscribe_log(
    log_path: &Option<PathBuf>,
    log_level: &String,
    max_log: usize,
) -> Option<WorkerGuard> {
    let subscriber_builder = FmtSubscriber::builder()
        .with_env_filter(log_level)
        .with_test_writer();
    match log_path {
        Some(ref log_path) => {
            let file_appender = Builder::new()
                .rotation(Rotation::DAILY)
                .filename_prefix("raiko.log")
                .max_log_files(max_log)
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
    }
}

/// Gets the config going through all possible sources
fn get_config(request_config: Option<Value>) -> Result<Value> {
    let mut config = Value::default();
    let opt = Opt::from_args();

    // Config file has the lowest preference
    let file = File::open(&opt.config_path)?;
    let reader = BufReader::new(file);
    let file_config: Value = serde_json::from_reader(reader)?;
    merge(&mut config, &file_config);

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
