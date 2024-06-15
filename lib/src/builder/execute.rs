// Copyright 2024 RISC Zero, Inc.
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

use alloy_consensus::{constants::BEACON_ROOTS_ADDRESS, TxEnvelope};
use alloy_primitives::{TxKind, U256};
use anyhow::{anyhow, bail, ensure, Context, Error, Result};
use core::{fmt::Debug, mem::take, str::from_utf8};
#[cfg(feature = "std")]
use log::debug;
use revm::{
    interpreter::Host,
    primitives::{
        Account, Address, EVMError, HandlerCfg, ResultAndState, SpecId, TransactTo, TxEnv,
        MAX_BLOB_GAS_PER_BLOCK,
    },
    taiko, Database, DatabaseCommit, Evm, JournaledState,
};
use std::collections::HashSet;
use tracing::trace;
cfg_if::cfg_if! {
    if #[cfg(feature = "tracer")] {
        use std::{fs::{OpenOptions, File}, io::{BufWriter, Write}, sync::{Arc, Mutex}};
        use revm::{inspector_handle_register, inspectors::TracerEip3155};
    }
}

use super::{OptimisticDatabase, TxExecStrategy};
use crate::{
    builder::BlockBuilder,
    clear_line,
    consts::GWEI_TO_WEI,
    guest_mem_forget, inplace_print,
    primitives::{
        alloy_eips::eip4788::SYSTEM_ADDRESS, mpt::MptNode, receipt::Receipt, Bloom, Rlp2718Bytes,
        RlpBytes,
    },
    print_duration,
    time::{AddAssign, Duration, Instant},
    utils::{check_anchor_tx, generate_transactions},
    CycleTracker,
};

/// Minimum supported protocol version: SHANGHAI
const MIN_SPEC_ID: SpecId = SpecId::SHANGHAI;

pub struct TkoTxExecStrategy {}

impl TxExecStrategy for TkoTxExecStrategy {
    fn execute_transactions<D>(mut block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit + OptimisticDatabase,
        <D as Database>::Error: Debug,
    {
        let mut tx_transact_duration = Duration::default();
        let mut tx_misc_duration = Duration::default();

        let is_optimistic = block_builder.db().unwrap().is_optimistic();

        let header = block_builder
            .header
            .as_mut()
            .expect("Header is not initialized");
        // Compute the spec id
        let spec_id = block_builder
            .chain_spec
            .active_fork(header.number, header.timestamp)
            .unwrap();
        if !SpecId::enabled(spec_id, MIN_SPEC_ID) {
            bail!("Invalid protocol version: expected >= {MIN_SPEC_ID:?}, got {spec_id:?}")
        }

        let chain_spec = &block_builder.input.chain_spec;
        let chain_id = chain_spec.chain_id();
        let is_taiko = chain_spec.is_taiko();
        trace!("spec_id: {spec_id:?}");

        // generate the transactions from the tx list
        // For taiko blocks, insert the anchor tx as the first transaction
        let anchor_tx = if chain_spec.is_taiko() {
            Some(serde_json::from_str(&block_builder.input.taiko.anchor_tx.clone()).unwrap())
        } else {
            None
        };
        let mut transactions = generate_transactions(
            chain_spec,
            block_builder.input.taiko.block_proposed.meta.blobUsed,
            &block_builder.input.taiko.tx_data,
            anchor_tx,
        );

        // Setup the EVM environment
        let evm = Evm::builder().with_db(block_builder.db.take().unwrap());
        #[cfg(feature = "tracer")]
        let evm = evm.with_external_context(TracerEip3155::new(Box::new(std::io::stdout())));
        let evm = evm
            .with_handler_cfg(HandlerCfg::new_with_taiko(spec_id, is_taiko))
            .modify_cfg_env(|cfg_env| {
                // set the EVM configuration
                cfg_env.chain_id = chain_id;
            })
            .modify_block_env(|blk_env| {
                // set the EVM block environment
                blk_env.number = U256::from(header.number);
                blk_env.coinbase = block_builder.input.beneficiary;
                blk_env.timestamp = header.timestamp.try_into().unwrap();
                blk_env.difficulty = U256::ZERO;
                blk_env.prevrandao = Some(header.mix_hash);
                blk_env.basefee = header.base_fee_per_gas.unwrap().try_into().unwrap();
                blk_env.gas_limit = block_builder.input.gas_limit.try_into().unwrap();
                if let Some(excess_blob_gas) = header.excess_blob_gas {
                    blk_env.set_blob_excess_gas_and_price(excess_blob_gas.try_into().unwrap());
                }
            });
        let evm = if is_taiko {
            evm.append_handler_register(taiko::handler_register::taiko_handle_register)
        } else {
            evm
        };
        #[cfg(feature = "tracer")]
        let evm = evm.append_handler_register(inspector_handle_register);
        let mut evm = evm.build();

        // Set the beacon block root in the EVM
        if spec_id >= SpecId::CANCUN {
            let parent_beacon_block_root = header.parent_beacon_block_root.unwrap();

            // From EIP-4788 Beacon block root in the EVM (Cancun):
            // "Call BEACON_ROOTS_ADDRESS as SYSTEM_ADDRESS with the 32-byte input of
            //  header.parent_beacon_block_root, a gas limit of 30_000_000, and 0 value."
            evm.env_mut().tx = TxEnv {
                transact_to: TransactTo::Call(BEACON_ROOTS_ADDRESS),
                caller: SYSTEM_ADDRESS,
                data: parent_beacon_block_root.into(),
                gas_limit: 30_000_000,
                value: U256::ZERO,
                ..Default::default()
            };

            let tmp = evm.env_mut().block.clone();

            // disable block gas limit validation and base fee checks
            evm.block_mut().gas_limit = U256::from(evm.tx().gas_limit);
            evm.block_mut().basefee = U256::ZERO;

            let ResultAndState { mut state, .. } =
                evm.transact().expect("beacon roots contract call failed");
            evm.env_mut().block = tmp;

            // commit only the changes to the beacon roots contract
            state.remove(&SYSTEM_ADDRESS);
            state.remove(&evm.block().coinbase);
            evm.context.evm.db.commit(state);
        }

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = 0u64;
        let mut blob_gas_used = 0_u64;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        // track the actual tx number to use in the tx/receipt trees as the key
        let mut actual_tx_no = 0usize;
        let num_transactions = transactions.len();
        for (tx_no, tx) in take(&mut transactions).into_iter().enumerate() {
            if !is_optimistic {
                cfg_if::cfg_if! {
                    if #[cfg(all(all(target_os = "zkvm", target_vendor = "succinct"), feature = "sp1-cycle-tracker"))]{
                        println!(
                            "{:?}",
                            &format!("\rprocessing tx {tx_no}/{num_transactions}...")
                        );
                    } else {
                        inplace_print(&format!("\rprocessing tx {tx_no}/{num_transactions}..."));
                    }
                }
            } else {
                trace!("\rprocessing tx {tx_no}/{num_transactions}...");
            }

            #[cfg(feature = "tracer")]
            let trace = set_trace_writer(
                &mut evm.context.external,
                chain_id,
                block_builder.input.block_number,
                actual_tx_no,
            );

            // anchor transaction always the first transaction
            let is_anchor = is_taiko && tx_no == 0;

            // setup the EVM environment
            let tx_env = &mut evm.env_mut().tx;
            fill_eth_tx_env(tx_env, &tx)?;
            // Set and check some taiko specific values
            if chain_spec.is_taiko() {
                // set if the tx is the anchor tx
                tx_env.taiko.is_anchor = is_anchor;
                // set the treasury address
                tx_env.taiko.treasury = chain_spec.l2_contract.unwrap_or_default();

                // Data blobs are not allowed on L2
                ensure!(tx_env.blob_hashes.len() == 0);
            }

            // if the signature was not valid, the caller address will have been set to zero
            if tx_env.caller == Address::ZERO {
                if is_anchor {
                    bail!("Error recovering anchor signature");
                }
                #[cfg(feature = "std")]
                debug!("Error recovering address for transaction {tx_no}");
                if !is_taiko {
                    bail!("invalid signature");
                }
                // If the signature is not valid, skip the transaction
                continue;
            }

            // verify the anchor tx
            if is_anchor {
                check_anchor_tx(&block_builder.input, &tx, &tx_env.caller)
                    .expect("invalid anchor tx");
            }

            // verify transaction gas
            let block_available_gas = block_builder.input.gas_limit - cumulative_gas_used;
            if block_available_gas < tx_env.gas_limit {
                if is_optimistic {
                    continue;
                }
                if is_anchor {
                    bail!("Error at transaction {tx_no}: gas exceeds block limit");
                }
                #[cfg(feature = "std")]
                debug!("Error at transaction {tx_no}: gas exceeds block limit");
                if !is_taiko {
                    bail!("gas exceeds block limit");
                }
                continue;
            }

            // verify blob gas
            if let TxEnvelope::Eip4844(blob_tx) = &tx {
                let tx = blob_tx.tx().tx();
                blob_gas_used = blob_gas_used.checked_add(tx.blob_gas()).unwrap();
                ensure!(
                    blob_gas_used <= MAX_BLOB_GAS_PER_BLOCK,
                    "Error at transaction {tx_no}: total blob gas spent exceeds the limit",
                );
            }

            // process the transaction
            let start = Instant::now();
            let cycle_tracker = CycleTracker::start("evm.transact()");
            let ResultAndState { result, state } = match evm.transact() {
                Ok(result) => result,
                Err(err) => {
                    // Clear the state for the next tx
                    evm.context.evm.journaled_state = JournaledState::new(spec_id, HashSet::new());

                    if is_optimistic {
                        continue;
                    }
                    if !is_taiko {
                        bail!("tx failed to execute successfully: {err:?}");
                    }
                    if is_anchor {
                        bail!("Anchor tx failed to execute successfully: {err:?}");
                    }
                    // only continue for invalid tx errors, not db errors (because those can be
                    // manipulated by the prover)
                    match err {
                        EVMError::Transaction(invalid_transaction) => {
                            #[cfg(feature = "std")]
                            debug!("Invalid tx at {tx_no}: {invalid_transaction:?}");
                            cycle_tracker.end();
                            // skip the tx
                            continue;
                        }
                        _ => {
                            // any other error is not allowed
                            bail!("Error at tx {tx_no}: {err:?}");
                        }
                    }
                }
            };
            cycle_tracker.end();
            #[cfg(feature = "std")]
            trace!("  Ok: {result:?}");

            #[cfg(feature = "tracer")]
            // Flush the trace writer
            trace.lock().unwrap().flush().expect("Error flushing trace");

            tx_transact_duration.add_assign(start.elapsed());

            let start = Instant::now();

            // anchor tx needs to succeed
            if is_anchor && !result.is_success() && !is_optimistic {
                bail!(
                    "Error at transaction {tx_no}: execute anchor failed {result:?}, output {:?}",
                    result.output().map(|o| from_utf8(o).unwrap_or_default())
                );
            }

            // keep track of all the gas used in the block
            let gas_used = result.gas_used();
            cumulative_gas_used = cumulative_gas_used.checked_add(gas_used).unwrap();

            // create the receipt from the EVM result
            let receipt = Receipt::new(
                tx.tx_type() as u8,
                result.is_success(),
                cumulative_gas_used.try_into().unwrap(),
                result.logs().iter().map(|log| log.clone().into()).collect(),
            );

            // update the state
            evm.context.evm.db.commit(state);

            // accumulate logs to the block bloom filter
            logs_bloom.accrue_bloom(&receipt.payload.logs_bloom);

            // Add receipt and tx to tries
            let trie_key = alloy_rlp::encode(actual_tx_no);
            // Add to tx trie
            tx_trie.insert_rlp_encoded(&trie_key, tx.to_rlp_2718())?;
            // Add to receipt trie
            receipt_trie.insert_rlp(&trie_key, receipt)?;

            // If we got here it means the tx is not invalid
            actual_tx_no += 1;

            tx_misc_duration.add_assign(start.elapsed());
        }
        if !is_optimistic {
            clear_line();
            print_duration("Tx transact time: ", tx_transact_duration);
            print_duration("Tx misc time: ", tx_misc_duration);
        }
        clear_line();
        println!("actual Tx: {}", actual_tx_no);

        let mut db = &mut evm.context.evm.db;

        // process withdrawals unconditionally after any transactions
        let mut withdrawals_trie = MptNode::default();
        for (i, withdrawal) in block_builder.input.withdrawals.iter().enumerate() {
            // the withdrawal amount is given in Gwei
            let amount_wei = GWEI_TO_WEI
                .checked_mul(withdrawal.amount.try_into().unwrap())
                .unwrap();

            // Credit withdrawal amount
            increase_account_balance(&mut db, withdrawal.address, amount_wei)?;
            // Add withdrawal to trie
            withdrawals_trie
                .insert_rlp(&i.to_rlp(), withdrawal)
                .with_context(|| "failed to insert withdrawal")?;
        }

        // Update result header with computed values
        header.transactions_root = tx_trie.hash();
        header.receipts_root = receipt_trie.hash();
        header.logs_bloom = logs_bloom;
        header.gas_used = cumulative_gas_used.into();
        if spec_id >= SpecId::SHANGHAI {
            header.withdrawals_root = Some(withdrawals_trie.hash());
        };
        if spec_id >= SpecId::CANCUN {
            header.blob_gas_used = Some(blob_gas_used.into());
        }

        // Leak memory, save cycles
        guest_mem_forget([tx_trie, receipt_trie, withdrawals_trie]);
        // Return block builder with updated database
        Ok(block_builder.with_db(evm.context.evm.inner.db))
    }
}

pub fn fill_eth_tx_env(tx_env: &mut TxEnv, tx: &TxEnvelope) -> Result<(), Error> {
    // Clear values that may not be set
    tx_env.access_list.clear();
    tx_env.blob_hashes.clear();
    tx_env.max_fee_per_blob_gas.take();
    // Get the data from the tx
    // TODO(Brecht): use optimized recover
    match tx {
        TxEnvelope::Legacy(tx) => {
            let cycle_tracker = CycleTracker::start("Legacy");
            tx_env.caller = tx.recover_signer().unwrap_or_default();
            cycle_tracker.end();
            let tx = tx.tx();
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.gas_price.try_into().unwrap();
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let TxKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = tx.chain_id;
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list.clear();
        }
        TxEnvelope::Eip2930(tx) => {
            let cycle_tracker = CycleTracker::start("Eip2930");
            tx_env.caller = tx.recover_signer().unwrap_or_default();
            cycle_tracker.end();
            let tx = tx.tx();
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.gas_price.try_into().unwrap();
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let TxKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.flattened();
        }
        TxEnvelope::Eip1559(tx) => {
            let cycle_tracker = CycleTracker::start("Eip1559");
            tx_env.caller = tx.recover_signer().unwrap_or_default();
            cycle_tracker.end();
            let tx = tx.tx();
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.max_fee_per_gas.try_into().unwrap();
            tx_env.gas_priority_fee = Some(tx.max_priority_fee_per_gas.try_into().unwrap());
            tx_env.transact_to = if let TxKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.flattened();
        }
        TxEnvelope::Eip4844(tx) => {
            let cycle_tracker = CycleTracker::start("Eip1559");
            tx_env.caller = tx.recover_signer().unwrap_or_default();
            cycle_tracker.end();
            let tx = tx.tx().tx();
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.max_fee_per_gas.try_into().unwrap();
            tx_env.gas_priority_fee = Some(tx.max_priority_fee_per_gas.try_into().unwrap());
            tx_env.transact_to = TransactTo::Call(tx.to);
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.flattened();
            tx_env.blob_hashes.clone_from(&tx.blob_versioned_hashes);
            tx_env.max_fee_per_blob_gas = Some(U256::from(tx.max_fee_per_blob_gas));
        }
        _ => todo!(),
    };
    Ok(())
}

pub fn increase_account_balance<D>(
    db: &mut D,
    address: Address,
    amount_wei: U256,
) -> anyhow::Result<()>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: Debug,
{
    // Read account from database
    let mut account: Account = db
        .basic(address)
        .map_err(|db_err| anyhow!("Error increasing account balance for {address}: {db_err:?}"))?
        .unwrap_or_default()
        .into();
    // Credit withdrawal amount
    account.info.balance = account.info.balance.checked_add(amount_wei).unwrap();
    account.mark_touch();
    // Commit changes to database
    db.commit([(address, account)].into());

    Ok(())
}

#[cfg(feature = "tracer")]
fn set_trace_writer(
    tracer: &mut TracerEip3155,
    chain_id: u64,
    block_number: u64,
    tx_no: usize,
) -> Arc<Mutex<BufWriter<File>>> {
    struct FlushWriter {
        writer: Arc<Mutex<BufWriter<std::fs::File>>>,
    }
    impl FlushWriter {
        fn new(writer: Arc<Mutex<BufWriter<std::fs::File>>>) -> Self {
            Self { writer }
        }
    }
    impl Write for FlushWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.writer.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.writer.lock().unwrap().flush()
        }
    }

    // Create the traces directory if it doesn't exist
    std::fs::create_dir_all("traces").expect("Failed to create traces directory");

    // Construct the file writer to write the trace to
    let file_name = format!("traces/{}_{}_{}.json", chain_id, block_number, tx_no);
    let write = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(file_name);
    let trace = Arc::new(Mutex::new(BufWriter::new(
        write.expect("Failed to open file"),
    )));
    let writer = FlushWriter::new(Arc::clone(&trace));

    // Set the writer inside the handler in revm
    tracer.set_writer(Box::new(writer));

    trace
}
