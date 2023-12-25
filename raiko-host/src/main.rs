use std::{fmt::Debug, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use raiko_host::{log::init_tracing, prover::server::serve};

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
    let _guard = init_tracing(
        args.max_log_days.expect("max_log_days not set"),
        &args.log_path,
        "raiko-host.log",
    );
    serve(
        &args.bind.unwrap(),
        &args.guest.unwrap(),
        &args.cache.unwrap(),
        &args.log_path,
        args.sgx_instance_id,
        args.proof_cache.unwrap(),
        args.concurrency_limit.unwrap(),
    )
    .await?;
    Ok(())
}
