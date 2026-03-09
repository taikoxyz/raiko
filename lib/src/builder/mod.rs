use core::mem;
use std::sync::Arc;
use std::sync::LazyLock;

use crate::builder::consensus::RaikoBeaconConsensus;
use crate::primitives::keccak::keccak;
use crate::primitives::mpt::StateAccount;
use crate::utils::txs::generate_transactions;
use crate::utils::txs::generate_transactions_for_batch_blocks;
use crate::{
    consts::MAX_BLOCK_HASH_AGE,
    guest_mem_forget,
    input::{GuestBatchInput, GuestInput},
    mem_db::{AccountState, DbAccount, MemDb},
    CycleTracker,
};
use alethia_reth_block::config::TaikoEvmConfig;
use alethia_reth_chainspec::hardfork::TaikoHardfork;
use alethia_reth_chainspec::reth_chainspec::ChainHardforks;
use alethia_reth_chainspec::reth_chainspec::ChainSpec;
use alethia_reth_chainspec::reth_chainspec::EthereumHardfork;
use alethia_reth_chainspec::reth_chainspec::ForkCondition;
use alethia_reth_chainspec::reth_chainspec::Hardfork;
use alethia_reth_chainspec::reth_chainspec::Hardforks;
use alethia_reth_chainspec::spec::TaikoChainSpec;
use alethia_reth_chainspec::TAIKO_DEVNET;
use alethia_reth_chainspec::TAIKO_MAINNET;
use alethia_reth_consensus::transaction::TaikoTxEnvelope;
use alethia_reth_evm::factory::TaikoEvmFactory;
use alethia_reth_evm::spec::TaikoSpecId;
use alethia_reth_primitives::TaikoBlock;
use alloy_primitives::map::HashMap;
use alloy_primitives::Address;
use alloy_primitives::Bytes;
use alloy_primitives::B256;
use alloy_primitives::U256;
use anyhow::{bail, ensure, Result};
use block_executor::TaikoWithOptimisticBlockExecutor;
use reth_consensus::{Consensus, HeaderValidator};
use reth_ethereum_consensus::validate_block_post_execution;
use reth_evm::block::BlockExecutionResult;
use reth_evm::execute::Executor;
use reth_evm::execute::{BlockExecutionOutput, ProviderError};
use reth_evm::Database;
use reth_primitives::Header;
use reth_primitives::RecoveredBlock;
use reth_primitives::SealedHeader;
use revm::primitives::KECCAK_EMPTY;
use revm::state::Account;
use revm::state::AccountInfo;
use revm::state::AccountStatus;
use revm::state::Bytecode;
use revm::state::EvmStorageSlot;
use revm::DatabaseCommit;
use tracing::{debug, info};

mod block_executor;
mod consensus;

/// Surge dev list of hardforks.
pub static SURGE_DEV_HARDFORKS: LazyLock<ChainHardforks> = LazyLock::new(|| {
    ChainHardforks::new(vec![
        (EthereumHardfork::Frontier.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Homestead.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Dao.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Tangerine.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::SpuriousDragon.boxed(),
            ForkCondition::Block(0),
        ),
        (EthereumHardfork::Byzantium.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::Constantinople.boxed(),
            ForkCondition::Block(0),
        ),
        (
            EthereumHardfork::Petersburg.boxed(),
            ForkCondition::Block(0),
        ),
        (EthereumHardfork::Istanbul.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Berlin.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::London.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::Paris.boxed(),
            ForkCondition::TTD {
                fork_block: None,
                total_difficulty: U256::from(0),
                activation_block_number: 0,
            },
        ),
        (
            EthereumHardfork::Shanghai.boxed(),
            ForkCondition::Timestamp(0),
        ),
        (
            TaikoHardfork::Ontake.boxed(),
            ForkCondition::Block(
                std::env::var("SURGE_DEV_ONTAKE_HEIGHT").map_or(1, |h| h.parse().unwrap_or(1)),
            ),
        ),
        (TaikoHardfork::Pacaya.boxed(), ForkCondition::Block(1)),
        (TaikoHardfork::Shasta.boxed(), ForkCondition::Timestamp(0)),
    ])
});

pub static SURGE_TEST_HARDFORKS: LazyLock<ChainHardforks> = LazyLock::new(|| {
    ChainHardforks::new(vec![
        (EthereumHardfork::Frontier.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Homestead.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Dao.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Tangerine.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::SpuriousDragon.boxed(),
            ForkCondition::Block(0),
        ),
        (EthereumHardfork::Byzantium.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::Constantinople.boxed(),
            ForkCondition::Block(0),
        ),
        (
            EthereumHardfork::Petersburg.boxed(),
            ForkCondition::Block(0),
        ),
        (EthereumHardfork::Istanbul.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Berlin.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::London.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::Paris.boxed(),
            ForkCondition::TTD {
                fork_block: None,
                total_difficulty: U256::from(0),
                activation_block_number: 0,
            },
        ),
        (
            EthereumHardfork::Shanghai.boxed(),
            ForkCondition::Timestamp(0),
        ),
        (
            TaikoHardfork::Ontake.boxed(),
            ForkCondition::Block(
                std::env::var("SURGE_TESTNET_ONTAKE_HEIGHT").map_or(1, |h| h.parse().unwrap_or(1)),
            ),
        ),
        (TaikoHardfork::Pacaya.boxed(), ForkCondition::Block(1)),
        (TaikoHardfork::Shasta.boxed(), ForkCondition::Timestamp(0)),
    ])
});

pub static SURGE_STAGE_HARDFORKS: LazyLock<ChainHardforks> = LazyLock::new(|| {
    ChainHardforks::new(vec![
        (EthereumHardfork::Frontier.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Homestead.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Dao.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Tangerine.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::SpuriousDragon.boxed(),
            ForkCondition::Block(0),
        ),
        (EthereumHardfork::Byzantium.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::Constantinople.boxed(),
            ForkCondition::Block(0),
        ),
        (
            EthereumHardfork::Petersburg.boxed(),
            ForkCondition::Block(0),
        ),
        (EthereumHardfork::Istanbul.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::Berlin.boxed(), ForkCondition::Block(0)),
        (EthereumHardfork::London.boxed(), ForkCondition::Block(0)),
        (
            EthereumHardfork::Paris.boxed(),
            ForkCondition::TTD {
                fork_block: None,
                total_difficulty: U256::from(0),
                activation_block_number: 0,
            },
        ),
        (
            EthereumHardfork::Shanghai.boxed(),
            ForkCondition::Timestamp(0),
        ),
        (
            TaikoHardfork::Ontake.boxed(),
            ForkCondition::Block(
                std::env::var("SURGE_STAGING_ONTAKE_HEIGHT").map_or(1, |h| h.parse().unwrap_or(1)),
            ),
        ),
        (TaikoHardfork::Pacaya.boxed(), ForkCondition::Block(1)),
        (TaikoHardfork::Shasta.boxed(), ForkCondition::Timestamp(0)),
    ])
});

pub static SURGE_DEV: LazyLock<Arc<TaikoChainSpec>> = LazyLock::new(|| {
    let hardforks = SURGE_DEV_HARDFORKS.clone();
    TaikoChainSpec {
        inner: ChainSpec {
            chain: 763374.into(), // TODO: make this dynamic based on the chain spec
            paris_block_and_final_difficulty: None,
            hardforks,
            deposit_contract: None,
            ..Default::default()
        },
    }
    .into()
});

pub static SURGE_STAGE: LazyLock<Arc<TaikoChainSpec>> = LazyLock::new(|| {
    let hardforks = SURGE_STAGE_HARDFORKS.clone();
    TaikoChainSpec {
        inner: ChainSpec {
            chain: 763373.into(), // TODO: make this dynamic based on the chain spec
            paris_block_and_final_difficulty: None,
            hardforks,
            deposit_contract: None,
            ..Default::default()
        },
    }
    .into()
});

pub static SURGE_TEST: LazyLock<Arc<TaikoChainSpec>> = LazyLock::new(|| {
    let hardforks = SURGE_TEST_HARDFORKS.clone();
    TaikoChainSpec {
        inner: ChainSpec {
            chain: 763375.into(), // TODO: make this dynamic based on the chain spec
            paris_block_and_final_difficulty: None,
            hardforks,
            deposit_contract: None,
            ..Default::default()
        },
    }
    .into()
});

pub static SURGE_MAINNET: LazyLock<Arc<TaikoChainSpec>> = LazyLock::new(|| {
    let hardforks = SURGE_STAGE_HARDFORKS.clone();
    TaikoChainSpec {
        inner: ChainSpec {
            chain: 763374.into(), // TODO: make this dynamic based on the chain spec
            paris_block_and_final_difficulty: None,
            hardforks,
            deposit_contract: None,
            ..Default::default()
        },
    }
    .into()
});

pub fn calculate_block_header(input: &mut GuestInput) -> Header {
    let cycle_tracker = CycleTracker::start("initialize_database");
    let db = create_mem_db(input).unwrap();
    cycle_tracker.end();

    let pool_tx = generate_transactions(
        &input.chain_spec,
        &input.taiko.block_proposed,
        &input.taiko.tx_data,
        &input.taiko.anchor_tx,
    );

    let guest_input = mem::take(input);
    let mut builder = RethBlockBuilder::new(guest_input, db);

    let cycle_tracker = CycleTracker::start("execute_transactions");
    builder
        .execute_transactions(pool_tx, false)
        .expect("execute");
    cycle_tracker.end();

    let cycle_tracker = CycleTracker::start("finalize");
    let header = builder.finalize().expect("execute");
    cycle_tracker.end();

    // Put the (partially consumed) input back so callers can still read
    // metadata (chain_spec, block, taiko, parent_header) after this function.
    *input = mem::take(&mut builder.input);
    header
}

pub fn calculate_batch_blocks_final_header(input: &mut GuestBatchInput) -> Vec<TaikoBlock> {
    let pool_txs_list = generate_transactions_for_batch_blocks(&input);
    let mut final_blocks = Vec::new();
    for (i, pool_txs) in pool_txs_list.iter().enumerate() {
        // First, create the MemDb using a mutable reference (no clone needed —
        // create_mem_db only mem::takes `contracts` and storage `slots`).
        let db = create_mem_db(&mut input.inputs[i]).unwrap();
        // Then, take ownership of the GuestInput for the builder (no clone needed —
        // parent_state_trie and parent_storage tries are still intact after create_mem_db).
        let guest_input = mem::take(&mut input.inputs[i]);
        let mut builder =
            RethBlockBuilder::new(guest_input, db).set_is_first_block_in_proposal(i == 0);

        let mut execute_tx = vec![builder.input.taiko.anchor_tx.clone().unwrap()];
        execute_tx.extend_from_slice(&pool_txs.0);
        builder
            .execute_transactions(execute_tx.clone(), false)
            .expect("execute");
        final_blocks.push(
            builder
                .finalize_block()
                .expect("execute single batched block"),
        );
        // Put the (partially consumed) input back so callers can still read
        // metadata (chain_spec, block, taiko, parent_header) after this function.
        // Only contracts, storage slots, and parent_state_trie have been consumed.
        input.inputs[i] = mem::take(&mut builder.input);
    }
    validate_final_batch_blocks(&final_blocks);

    final_blocks
}

// to check the linkages between the blocks
// 1. connect parent hash & state root
// 2. block number should be in sequence
// Note: state_root linkage is already validated by create_mem_db which asserts
// parent_state_trie.hash() == parent_header.state_root for each block.
fn validate_final_batch_blocks(final_blocks: &[TaikoBlock]) {
    final_blocks.windows(2).for_each(|window| {
        let parent_block = &window[0];
        let current_block = &window[1];
        let calculated_parent_hash = parent_block.header.hash_slow();
        assert!(
            calculated_parent_hash == current_block.header.parent_hash,
            "Parent hash mismatch, expected: {}, got: {}",
            calculated_parent_hash,
            current_block.header.parent_hash
        );
        assert!(
            parent_block.header.number + 1 == current_block.header.number,
            "Block number mismatch, expected: {}, got: {}",
            parent_block.header.number + 1,
            current_block.header.number
        );
    });
}

/// Optimistic database
#[allow(async_fn_in_trait)]
pub trait OptimisticDatabase {
    /// Handle post execution work
    async fn fetch_data(&mut self) -> bool;

    /// If the current database is optimistic
    fn is_optimistic(&self) -> bool;
}
/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct RethBlockBuilder<DB> {
    pub chain_spec: crate::consts::ChainSpec,
    pub input: GuestInput,
    pub db: Option<DB>,
    /// Whether this is the first block in a proposal batch (for Shasta)
    pub is_first_block_in_proposal: bool,
}

impl<DB: Database<Error = ProviderError> + DatabaseCommit + OptimisticDatabase + Clone>
    RethBlockBuilder<DB>
{
    /// Creates a new block builder.
    /// For single block execution, `is_first_block_in_proposal` defaults to `true`.
    /// For batch execution, it should be set explicitly using `set_is_first_block_in_proposal`.
    /// Takes ownership of the GuestInput to avoid expensive deep clones.
    pub fn new(input: GuestInput, db: DB) -> RethBlockBuilder<DB> {
        RethBlockBuilder {
            chain_spec: input.chain_spec.clone(),
            db: Some(db),
            input,
            is_first_block_in_proposal: true, // Default to true for single block execution
        }
    }

    /// Sets whether this is the first block in a proposal batch.
    pub fn set_is_first_block_in_proposal(mut self, is_first: bool) -> Self {
        self.is_first_block_in_proposal = is_first;
        self
    }

    /// Executes all input transactions.
    pub fn execute_transactions(
        &mut self,
        pool_txs: Vec<TaikoTxEnvelope>,
        optimistic: bool,
    ) -> Result<()> {
        info!("execute_transactions: start");
        // Get the chain spec
        let chain_spec = &self.input.chain_spec;
        let chain_spec = match chain_spec.name.as_str() {
            "taiko_mainnet" => TAIKO_MAINNET.clone(),
            "taiko_dev" => TAIKO_DEVNET.clone(),
            "surge_dev" => SURGE_DEV.clone(),
            "surge_test" => SURGE_TEST.clone(),
            "surge_stage" => SURGE_STAGE.clone(),
            "surge_mainnet" => SURGE_MAINNET.clone(),
            _ => unimplemented!(),
        };

        info!("execute_transactions: reth_chain_spec done");

        // todo: shasta has decouple the connection between proposal & block id.
        // need constraint for it.
        let block_num = self.input.block.number;
        let block_ts = self.input.block.timestamp;
        let taiko_fork = self.input.chain_spec.spec_id(block_num, block_ts).unwrap();

        match taiko_fork {
            TaikoSpecId::ONTAKE => {
                assert!(
                    chain_spec
                        .fork(TaikoHardfork::Ontake)
                        .active_at_block(block_num),
                    "evm fork ONTAKE is not active, please update the chain spec"
                );
            }
            TaikoSpecId::PACAYA => {
                assert!(
                    chain_spec
                        .fork(TaikoHardfork::Pacaya)
                        .active_at_block(block_num),
                    "evm fork PACAYA is not active, please update the chain spec"
                );
            }
            TaikoSpecId::SHASTA => {
                // shasta is activated by timestamp, not block number
                assert!(
                    chain_spec
                        .fork(TaikoHardfork::Shasta)
                        .active_at_timestamp(block_ts),
                    "evm fork SHASTA is not active, please update the chain spec"
                );
            }
            _ => unimplemented!(),
        }
        info!("execute_transactions: is_taiko done");

        // Generate the transactions from the tx list
        let mut block = self.input.block.clone();
        block.body.transactions = pool_txs;

        // let shasta_data_opt = if let Some(extra_data) = &self.input.taiko.extra_data {
        //     let last_anchor_block_number_opt =
        //         self.input.taiko.prover_data.last_anchor_block_number;
        //     assert!(
        //         last_anchor_block_number_opt.is_some(),
        //         "last_anchor_block_number is not set in shasta request"
        //     );
        //     Some(ShastaData {
        //         proposal_id: self.input.taiko.block_proposed.proposal_id(),
        //         is_low_bond_proposal: extra_data.0,
        //         designated_prover: extra_data.1,
        //         last_anchor_block_number: last_anchor_block_number_opt.unwrap(),
        //         is_force_inclusion: extra_data.2,
        //     })
        // } else {
        //     None
        // };

        let taiko_evm_config = TaikoEvmConfig::new_with_evm_factory(
            chain_spec.clone(),
            TaikoEvmFactory::new(Some(Address::ZERO)), // TODO: make it configurable
        );

        // TODO: Maybe remove as "prover" feature has been added to taiko-reth?
        let executor = TaikoWithOptimisticBlockExecutor::new(
            taiko_evm_config,
            self.db.take().unwrap(),
            optimistic,
        );

        // Recover senders
        let recovered_block = RecoveredBlock::try_recover(block)?;

        let mut tmp_db = None;
        let BlockExecutionOutput {
            state,
            result:
                BlockExecutionResult {
                    receipts,
                    requests,
                    gas_used: _,
                    blob_gas_used: _,
                },
        } = executor.execute_with_state_closure(&recovered_block, |state| {
            tmp_db = Some(state.database.clone());
        })?;

        info!("execute_transactions: execute done");

        // Filter out the valid transactions so that the header checks only take these into account
        let mut block = recovered_block.into_block();

        let (filtered_txs, _): (Vec<_>, Vec<_>) = block
            .body
            .transactions
            .into_iter()
            .zip(receipts.clone())
            .filter(|(_, receipt)| receipt.success || (!receipt.success && optimistic))
            .unzip();

        block.body.transactions = filtered_txs;

        let recovered_block = RecoveredBlock::try_recover(block)?;
        let sealed_block = recovered_block.sealed_block();
        let sealed_header = sealed_block.sealed_header();

        info!("execute_transactions: valid_transaction_indices done");
        // Header validation
        if !optimistic {
            let consensus = RaikoBeaconConsensus::new(
                chain_spec.clone(),
                self.input.taiko.grandparent_timestamp,
            );

            // Validates if some values are set that should not be set for the current HF
            consensus.validate_header(sealed_header)?;
            info!("execute_transactions: validate_header done");

            // Validates parent block hash, block number and timestamp
            let parent_sealed_header = SealedHeader::new_unhashed(self.input.parent_header.clone());
            consensus.validate_header_against_parent(sealed_header, &parent_sealed_header)?;
            info!("execute_transactions: validate_header_against_parent done");

            // Validates ommers hash, transaction root, withdrawals root
            consensus.validate_block_pre_execution(sealed_block)?;
            info!("execute_transactions: validate_block_pre_execution done");

            // Validates the gas used, the receipts root and the logs bloom
            validate_block_post_execution(
                &recovered_block,
                &chain_spec,
                &receipts,
                &requests,
                None,
            )?;
            info!("execute_transactions: validate_block_post_execution done");
        }

        // Apply DB change
        self.db = tmp_db;
        info!("execute_transactions: changes start");
        let changes: HashMap<Address, Account> = state
            .state
            .into_iter()
            .map(|(address, bundle_account)| {
                let is_original_none = bundle_account.original_info.is_none();
                let is_info_none = bundle_account.info.is_none();
                let account_info = bundle_account.account_info().unwrap_or_default();
                let original_info = bundle_account.original_info.unwrap_or_default();

                let mut account = Account {
                    original_info: Box::new(original_info),
                    info: account_info,
                    storage: bundle_account
                        .storage
                        .into_iter()
                        .map(|(k, v)| {
                            (
                                k,
                                EvmStorageSlot {
                                    original_value: v.original_value(),
                                    present_value: v.present_value(),
                                    transaction_id: 0,
                                    // is_cold used in EIP-2929 for optimizing gas costs for slot accesses, we don't need this in proving
                                    is_cold: false,
                                },
                            )
                        })
                        .collect(),
                    status: AccountStatus::default(),
                    transaction_id: 0,
                };
                account.mark_touch();
                if is_info_none {
                    account.mark_selfdestruct();
                }
                if is_original_none {
                    account.mark_created();
                }
                (address, account)
            })
            .collect();
        self.db.as_mut().unwrap().commit(changes);
        info!("execute_transactions: commit done");
        Ok(())
    }
}

impl RethBlockBuilder<MemDb> {
    /// Finalizes the block building and returns the header
    pub fn finalize(&mut self) -> Result<Header> {
        let state_root = self.calculate_state_root()?;
        ensure!(self.input.block.state_root == state_root);
        Ok(self.input.block.header.clone())
    }

    /// Finalizes the block building and returns the header
    pub fn finalize_block(&mut self) -> Result<TaikoBlock> {
        let state_root = self.calculate_state_root()?;
        assert_eq!(self.input.block.state_root, state_root);
        ensure!(self.input.block.state_root == state_root);
        Ok(self.input.block.clone())
    }

    /// Calculates the state root of the block
    pub fn calculate_state_root(&mut self) -> Result<B256> {
        let mut account_touched = 0;
        let mut storage_touched = 0;

        // apply state updates
        let mut state_trie = mem::take(&mut self.input.parent_state_trie);
        for (address, account) in &self.db.as_ref().unwrap().accounts {
            // if the account has not been touched, it can be ignored
            if account.state == AccountState::None {
                continue;
            }

            // compute the index of the current account in the state trie
            let state_trie_index = keccak(address);

            // remove deleted accounts from the state trie
            if account.state == AccountState::Deleted {
                state_trie.delete(&state_trie_index)?;
                continue;
            }

            account_touched += 1;

            // otherwise, compute the updated storage root for that account
            let state_storage = &account.storage;
            let storage_root = {
                // getting a mutable reference is more efficient than calling remove
                // every account must have an entry, even newly created accounts
                let (storage_trie, _) = self
                    .input
                    .parent_storage
                    .get_mut(address)
                    .expect("Address not found in storage");
                // for cleared accounts always start from the empty trie
                if account.state == AccountState::StorageCleared {
                    storage_trie.clear();
                }

                // apply all new storage entries for the current account (address)
                for (key, value) in state_storage {
                    let storage_trie_index = keccak(key.to_be_bytes::<32>());
                    if value.is_zero() {
                        storage_trie.delete(&storage_trie_index)?;
                    } else {
                        storage_trie.insert_rlp(&storage_trie_index, *value)?;
                    }
                }

                storage_touched += 1;

                storage_trie.hash()
            };

            let state_account = StateAccount {
                nonce: account.info.nonce,
                balance: account.info.balance,
                storage_root,
                code_hash: account.info.code_hash,
            };
            state_trie.insert_rlp(&state_trie_index, state_account)?;
        }

        debug!("Accounts touched {account_touched:?}");
        debug!("Storages touched {storage_touched:?}");

        Ok(state_trie.hash())
    }
}

pub fn create_mem_db(input: &mut GuestInput) -> Result<MemDb> {
    // Verify state trie root
    if input.parent_state_trie.hash() != input.parent_header.state_root {
        bail!(
            "Invalid state trie: expected {}, got {}",
            input.parent_header.state_root,
            input.parent_state_trie.hash()
        );
    }

    // hash all the contract code
    let contracts: HashMap<B256, Bytes> = mem::take(&mut input.contracts)
        .into_iter()
        .map(|bytes| (keccak(&bytes).into(), bytes))
        .collect();

    let mut account_touched = 0;
    let mut storage_touched = 0;

    // Load account data into db
    let mut accounts = HashMap::with_capacity(input.parent_storage.len());
    for (address, (storage_trie, slots)) in &mut input.parent_storage {
        // consume the slots, as they are no longer needed afterwards
        let slots = mem::take(slots);

        account_touched += 1;

        // load the account from the state trie or empty if it does not exist
        let state_account = input
            .parent_state_trie
            .get_rlp::<StateAccount>(&keccak(address))?
            .unwrap_or_default();
        // Verify storage trie root
        if storage_trie.hash() != state_account.storage_root {
            bail!(
                "Invalid storage trie for {address:?}: expected {}, got {}",
                state_account.storage_root,
                storage_trie.hash()
            );
        }

        // load the corresponding code
        let code_hash = state_account.code_hash;
        let bytecode = if code_hash.0 == KECCAK_EMPTY.0 {
            Bytecode::new()
        } else {
            let bytes: Bytes = contracts
                .get(&code_hash)
                .expect(&format!("Contract {code_hash} of {address} exists"))
                .clone();
            Bytecode::new_raw(bytes)
        };

        // load storage reads
        let mut storage = HashMap::with_capacity(slots.len());
        for slot in slots {
            let value: U256 = storage_trie
                .get_rlp(&keccak(slot.to_be_bytes::<32>()))?
                .unwrap_or_default();
            storage.insert(slot, value);

            storage_touched += 1;
        }

        let mem_account = DbAccount {
            info: AccountInfo {
                account_id: None,
                balance: state_account.balance,
                nonce: state_account.nonce,
                code_hash: state_account.code_hash,
                code: Some(bytecode),
            },
            state: AccountState::None,
            storage,
        };

        accounts.insert(*address, mem_account);
    }
    guest_mem_forget(contracts);

    debug!("Accounts touched: {account_touched:?}");
    debug!("Storages touched: {storage_touched:?}");

    // prepare block hash history
    let mut block_hashes = HashMap::with_capacity(input.ancestor_headers.len() + 1);
    block_hashes.insert(input.parent_header.number, input.parent_header.hash_slow());
    let mut prev = &input.parent_header;
    for current in &input.ancestor_headers {
        let current_hash = current.hash_slow();
        if prev.parent_hash != current_hash {
            bail!(
                "Invalid chain: {} is not the parent of {}",
                current.number,
                prev.number
            );
        }
        if input.parent_header.number < current.number
            || input.parent_header.number - current.number >= MAX_BLOCK_HASH_AGE
        {
            bail!(
                "Invalid chain: {} is not one of the {MAX_BLOCK_HASH_AGE} most recent blocks",
                current.number,
            );
        }
        block_hashes.insert(current.number, current_hash);
        prev = current;
    }

    // Store database
    Ok(MemDb {
        accounts,
        block_hashes,
    })
}
