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

use std::{fmt::Debug, time::Instant};

use anyhow::{bail, Result};
use bonsai_sdk::alpha as bonsai_sdk;
use clap::Parser;
use ethers_core::types::Transaction as EthersTransaction;
use log::{error, info};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use zeth_lib::{
    block_builder::{BlockBuilder, NetworkStrategyBundle, TaikoStrategyBundle},
    consts::{ChainSpec, TAIKO_MAINNET_CHAIN_SPEC},
    finalization::DebugBuildFromMemDbStrategy,
    initialization::MemDbInitStrategy,
    input::Input,
};
use zeth_primitives::taiko::ProtocolInstance;
use zeth_primitives::BlockHash;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long, require_equals = true)]
    /// URL of the chain RPC node.
    rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: host/testdata]
    cache: Option<String>,

    #[clap(short, long, require_equals = true)]
    /// Block number to validate.
    block_no: u64,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "20")]
    /// Runs the verification inside the zkvm executor locally. Accepts a custom maximum
    /// segment cycle count as a power of 2. [default: 20]
    local_exec: Option<usize>,

    #[clap(short, long, require_equals = true)]
    /// protocol instance json format
    protocol_instance: String,
}

fn cache_file_path(cache_path: &String, network: &str, block_no: u64, ext: &str) -> String {
    format!("{}/{}/{}.{}", cache_path, network, block_no, ext)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    run_with_bundle::<TaikoStrategyBundle>(args, TAIKO_MAINNET_CHAIN_SPEC.clone()).await
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

    // TODO
    let pi = serde_json::from_str(args.protocol_instance.as_str())
        .expect("Could not parse protocol instance");

    let init_spec = chain_spec.clone();
    let init = tokio::task::spawn_blocking(move || {
        zeth_lib::host::get_initial_data::<N>(init_spec, rpc_cache, args.rpc_url, args.block_no, pi)
            .expect("Could not init")
    })
    .await?;

    let input: Input<N::TxEssence> = init.clone().into();

    // Verify that the transactions run correctly
    {
        info!("Running from memory ...");

        // todo: extend to use [ConfiguredBlockBuilder]
        let block_builder = BlockBuilder::new(&chain_spec, input.clone())
            .initialize_database::<MemDbInitStrategy>()
            .expect("Error initializing MemDb from Input")
            .prepare_header::<N::HeaderPrepStrategy>()
            .expect("Error creating initial block header")
            .execute_transactions::<N::TxExecStrategy>()
            .expect("Error while running transactions");

        let fini_db = block_builder.db().unwrap().clone();
        let accounts_len = fini_db.accounts_len();

        let (validated_header, storage_deltas) = block_builder
            .build::<DebugBuildFromMemDbStrategy>()
            .expect("Error while verifying final state");

        info!(
            "Memory-backed execution is Done! Database contains {} accounts",
            accounts_len
        );

        // Verify final state
        info!("Verifying final state using provider data ...");
        let errors = zeth_lib::host::verify_state(fini_db, init.fini_proofs, storage_deltas)
            .expect("Could not verify final state!");
        for (address, address_errors) in &errors {
            error!(
                "Verify found {:?} error(s) for address {:?}",
                address_errors.len(),
                address
            );
            for error in address_errors {
                match error {
                    zeth_lib::host::VerifyError::BalanceMismatch {
                        rpc_value,
                        our_value,
                        difference,
                    } => error!(
                        "  Error: BalanceMismatch: rpc_value={} our_value={} difference={}",
                        rpc_value, our_value, difference
                    ),
                    _ => error!("  Error: {:?}", error),
                }
            }
        }

        let errors_len = errors.len();
        if errors_len > 0 {
            error!(
                "Verify found {:?} account(s) with error(s) ({}% correct)",
                errors_len,
                (100.0 * (accounts_len - errors_len) as f64 / accounts_len as f64)
            );
        }

        if validated_header.base_fee_per_gas != init.fini_block.base_fee_per_gas {
            error!(
                "Base fee mismatch {} (expected {})",
                validated_header.base_fee_per_gas, init.fini_block.base_fee_per_gas
            );
        }

        if validated_header.state_root != init.fini_block.state_root {
            error!(
                "State root mismatch {} (expected {})",
                validated_header.state_root, init.fini_block.state_root
            );
        }

        if validated_header.transactions_root != init.fini_block.transactions_root {
            error!(
                "Transactions root mismatch {} (expected {})",
                validated_header.transactions_root, init.fini_block.transactions_root
            );
        }

        if validated_header.receipts_root != init.fini_block.receipts_root {
            error!(
                "Receipts root mismatch {} (expected {})",
                validated_header.receipts_root, init.fini_block.receipts_root
            );
        }

        if validated_header.withdrawals_root != init.fini_block.withdrawals_root {
            error!(
                "Withdrawals root mismatch {:?} (expected {:?})",
                validated_header.withdrawals_root, init.fini_block.withdrawals_root
            );
        }

        let found_hash = validated_header.hash();
        let expected_hash = init.fini_block.hash();
        if found_hash.as_slice() != expected_hash.as_slice() {
            error!(
                "Final block hash mismatch {} (expected {})",
                found_hash, expected_hash,
            );

            bail!("Invalid block hash");
        }

        info!("Final block hash derived successfully. {}", found_hash)
    }

    Ok(())
}
