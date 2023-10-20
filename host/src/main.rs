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

extern crate core;

use std::fmt::Debug;

use anyhow::Result;
use clap::Parser;
use zeth_lib::{
    block_builder::EthereumStrategyBundle,
    consts::{Network, ETH_MAINNET_CHAIN_SPEC},
};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long, require_equals = true)]
    /// URL of the chain RPC node.
    rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata")]
    cache: Option<String>,

    #[clap(
        short,
        long,
        require_equals = true,
        value_enum,
        default_value = "ethereum"
    )]
    /// Network name.
    network: Network,

    #[clap(short, long, require_equals = true)]
    /// Block number to validate.
    block_no: u64,
}

fn cache_file_path(cache_path: &String, network: &String, block_no: u64, ext: &str) -> String {
    format!("{}/{}/{}.{}", cache_path, network, block_no, ext)
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

    let rpc_cache = args
        .cache
        .as_ref()
        .map(|dir| cache_file_path(dir, &args.network.to_string(), args.block_no, "json.gz"));

    tokio::task::spawn_blocking(move || {
        zeth_lib::host::get_initial_data::<EthereumStrategyBundle>(
            ETH_MAINNET_CHAIN_SPEC.clone(),
            rpc_cache,
            args.rpc_url,
            args.block_no,
        )
        .expect("Could not init")
    })
    .await?;

    Ok(())
}
