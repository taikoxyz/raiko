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

use std::fmt::Debug;

use anyhow::Result;
use clap::Parser;
use ethers_core::types::Transaction as EthersTransaction;
use serde::{Deserialize, Serialize};
use zeth_lib::{
    block_builder::{EthereumStrategyBundle, NetworkStrategyBundle},
    consts::{ChainSpec, ETH_MAINNET_CHAIN_SPEC},
};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long, require_equals = true)]
    /// Server bind address, e.g. "0.0.0.0:8080"
    bind: String,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "raiko-host/testdata")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: raiko-host/testdata]
    cache: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "raiko-host/guests")]
    /// The guests path
    /// [default: raiko-host/guests]
    guest: Option<String>,
}

fn cache_file_path(cache_path: &String, network: &str, block_no: u64, ext: &str) -> String {
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
    env_logger::init();
    let args = Args::parse();

    run_with_bundle::<EthereumStrategyBundle>(args, ETH_MAINNET_CHAIN_SPEC.clone()).await
}

async fn run_with_bundle<N: NetworkStrategyBundle>(args: Args, chain_spec: ChainSpec) -> Result<()>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
    <N::Database as revm::primitives::db::Database>::Error: Debug,
{
    // Fetch all of the initial data
    let rpc_cache = args
        .cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "taiko", args.block_no, "json.gz"));

    let init_spec = chain_spec.clone();
    // let _protocol_instance = protocol_instance.clone();
    let _init = tokio::task::spawn_blocking(move || {
        zeth_lib::host::get_initial_data::<N>(init_spec, rpc_cache, args.rpc_url, args.block_no)
            .expect("Could not init")
    })
    .await?;

    Ok(())
}
