#![feature(path_file_prefix)]
#![feature(absolute_path)]
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

mod prover;
mod rolling;
use std::{fmt::Debug, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use prover::server::serve;
use tracing::info;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(
        long,
        require_equals = true,
        num_args = 0..=1,
        default_value = "0.0.0.0:8080"
    )]
    /// Server bind address
    /// [default: 0.0.0.0:8080]
    bind: Option<String>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "/tmp")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    cache: Option<PathBuf>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "raiko-host/guests")]
    /// The guests path
    guest: Option<PathBuf>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "0")]
    sgx_instance_id: u32,

    #[clap(long, require_equals = true, num_args = 0..=1)]
    log_path: Option<PathBuf>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "1000")]
    proof_cache: Option<usize>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "10")]
    concurrency_limit: Option<usize>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "7")]
    max_log_days: Option<usize>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "internal_devnet_a")]
    l2_chain: Option<String>,

    #[clap(long, require_equals = true, num_args = 0..=1, default_value = "10")]
    max_caches: Option<usize>,
}

// Prerequisites:
//
//   $ rustup default
//   nightly-x86_64-unknown-linux-gnu (default)
//
// Go to /host directory and compile with:
//   $ cargo build
//
// Create /tmp/ethereum directory and run with:
//
//   $ RUST_LOG=info cargo run -- --rpc-url="https://rpc.internal.taiko.xyz/" --block-no=169 --cache=/tmp
//
// from target/debug directory

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    const DEFAULT_FILTER: &str = "info";
    // try to load filter from `RUST_LOG` or use reasonably verbose defaults
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| DEFAULT_FILTER.into());
    let subscriber_builder = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_test_writer();
    let _guard = match args.log_path {
        Some(ref log_path) => {
            let file_appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("raiko.log")
                .max_log_files(args.max_log_days.expect("max_log_days not set"))
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
    info!("Start args: {:?}", args);
    serve(
        &args.bind.unwrap(),
        &args.guest.unwrap(),
        &args.cache.unwrap(),
        &args.l2_chain.unwrap(),
        args.sgx_instance_id,
        args.proof_cache.unwrap(),
        args.concurrency_limit.unwrap(),
        args.max_caches.unwrap(),
    )
    .await?;
    Ok(())
}
