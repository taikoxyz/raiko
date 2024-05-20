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

use core::mem;

use alloy_consensus::Header as AlloyConsensusHeader;
use alloy_primitives::uint;
use anyhow::{bail, Context, Error, Result};
use raiko_primitives::{keccak::keccak, mpt::{MptNode, StateAccount}};
use reth_evm::execute::EthBlockOutput;
use reth_evm_ethereum::execute::EthExecutorProvider;
use reth_interfaces::executor::BlockValidationError;
use reth_primitives::{ChainSpecBuilder, MAINNET, U256};
use reth_provider::OriginalValuesKnown;
use revm::{Database, DatabaseCommit};
use reth_evm::execute::Executor;

pub use self::execute::TkoTxExecStrategy;
use self::initialize::create_db;
use crate::{
    builder::{
        finalize::{BlockFinalizeStrategy, MemDbBlockFinalizeStrategy},
        initialize::{DbInitStrategy, MemDbInitStrategy},
        prepare::{HeaderPrepStrategy, TaikoHeaderPrepStrategy},
    }, consts::ChainSpec, guest_mem_forget, input::GuestInput, mem_db::{AccountState, MemDb}, utils::HeaderHasher
};

pub mod execute;
pub mod finalize;
pub mod initialize;
pub mod prepare;

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
pub struct BlockBuilder<D> {
    pub(crate) chain_spec: ChainSpec,
    pub(crate) input: GuestInput,
    pub(crate) db: Option<D>,
    pub(crate) header: Option<AlloyConsensusHeader>,
}

impl<D> BlockBuilder<D>
where
    D: Database + DatabaseCommit + OptimisticDatabase,
    <D as Database>::Error: core::fmt::Debug,
{
    /// Creates a new block builder.
    pub fn new(input: &GuestInput) -> BlockBuilder<D> {
        BlockBuilder {
            chain_spec: input.chain_spec.clone(),
            db: None,
            header: None,
            input: input.clone(),
        }
    }

    /// Sets the database instead of initializing it from the input.
    pub fn with_db(mut self, db: D) -> Self {
        self.db = Some(db);
        self
    }

    /// Initializes the database from the input.
    pub fn initialize_database<T: DbInitStrategy<D>>(self) -> Result<Self> {
        T::initialize_database(self)
    }

    /// Initializes the header. This must be called before executing transactions.
    pub fn prepare_header<T: HeaderPrepStrategy>(self) -> Result<Self> {
        T::prepare_header(self)
    }

    /// Executes all input transactions.
    pub fn execute_transactions<T: TxExecStrategy>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    /// Finalizes the block building and returns the header and the state trie.
    pub fn finalize<T: BlockFinalizeStrategy<D>>(self) -> Result<(AlloyConsensusHeader, MptNode)> {
        T::finalize(self)
    }

    /// Returns a reference to the database.
    pub fn db(&self) -> Option<&D> {
        self.db.as_ref()
    }

    /// Returns a mutable reference to the database.
    pub fn mut_db(&mut self) -> Option<&mut D> {
        self.db.as_mut()
    }
}

/// A bundle of strategies for building a block using [BlockBuilder].
pub trait BlockBuilderStrategy {
    type DbInitStrategy: DbInitStrategy<MemDb>;
    type HeaderPrepStrategy: HeaderPrepStrategy;
    type TxExecStrategy: TxExecStrategy;
    type BlockFinalizeStrategy: BlockFinalizeStrategy<MemDb>;

    /// Builds a block from the given input.
    fn build_from(input: &GuestInput) -> Result<(AlloyConsensusHeader, MptNode)> {
        BlockBuilder::<MemDb>::new(input)
            .initialize_database::<Self::DbInitStrategy>()?
            .prepare_header::<Self::HeaderPrepStrategy>()?
            .execute_transactions::<Self::TxExecStrategy>()?
            .finalize::<Self::BlockFinalizeStrategy>()
    }
}

/// The [BlockBuilderStrategy] for building a Taiko block.
pub struct TaikoStrategy {}
impl BlockBuilderStrategy for TaikoStrategy {
    type DbInitStrategy = MemDbInitStrategy;
    type HeaderPrepStrategy = TaikoHeaderPrepStrategy;
    type TxExecStrategy = TkoTxExecStrategy;
    type BlockFinalizeStrategy = MemDbBlockFinalizeStrategy;
}
pub trait TxExecStrategy {
    fn execute_transactions<D>(block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit + OptimisticDatabase,
        <D as Database>::Error: core::fmt::Debug;
}

/// Multiplier for converting gwei to wei.
pub const GWEI_TO_WEI: U256 = uint!(1_000_000_000_U256);

/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct RethBlockBuilder {
    pub(crate) chain_spec: ChainSpec,
    pub(crate) input: GuestInput,
    pub(crate) db: MemDb,
    pub(crate) header: Option<AlloyConsensusHeader>,
}

impl RethBlockBuilder {
    /// Creates a new block builder.
    pub fn new(input: &GuestInput) -> RethBlockBuilder {
        let db = create_db(&mut input.clone()).unwrap();
        RethBlockBuilder {
            chain_spec: input.chain_spec.clone(),
            db,
            header: None,
            input: input.clone(),
        }
    }

    /// Initializes the header. This must be called before executing transactions.
    pub fn prepare_header(&mut self) -> Result<()> {
        /// Maximum size of extra data.
        pub const MAX_EXTRA_DATA_BYTES: usize = 32;

        // Validate timestamp
        let timestamp: u64 = self.input.timestamp;
        if timestamp < self.input.parent_header.timestamp {
            bail!(
                "Invalid timestamp: expected >= {}, got {}",
                self.input.parent_header.timestamp,
                self.input.timestamp,
            );
        }
        // Validate extra data
        let extra_data_bytes = self.input.extra_data.len();
        if extra_data_bytes > MAX_EXTRA_DATA_BYTES {
            bail!("Invalid extra data: expected <= {MAX_EXTRA_DATA_BYTES}, got {extra_data_bytes}")
        }
        // Derive header
        let number: u64 = self.input.parent_header.number;
        self.header = Some(AlloyConsensusHeader {
            // Initialize fields that we can compute from the parent
            parent_hash: self.input.parent_header.hash(),
            number: number
                .checked_add(1)
                .with_context(|| "Invalid block number: too large")?,
            base_fee_per_gas: Some(self.input.base_fee_per_gas.into()),
            // Initialize metadata from input
            beneficiary: self.input.beneficiary,
            gas_limit: self.input.gas_limit.into(),
            timestamp: self.input.timestamp,
            mix_hash: self.input.mix_hash,
            extra_data: self.input.extra_data.clone(),
            blob_gas_used: self.input.blob_gas_used.map(|b| b.into()),
            excess_blob_gas: self.input.excess_blob_gas.map(|b| b.into()),
            parent_beacon_block_root: self.input.parent_beacon_block_root,
            // Verified in reth
            receipts_root: self.input.block_header_reference.receipts_root,
            logs_bloom: self.input.block_header_reference.logs_bloom,
            // actually not sure if this is directly verified against header value
            gas_used: self.input.block_header_reference.gas_used,
            // TODO:
            transactions_root: self.input.block_header_reference.transactions_root,
            withdrawals_root: self.input.block_header_reference.withdrawals_root,
            // do not fill the remaining fields
            ..Default::default()
        });
        Ok(())
    }

    /// Executes all input transactions.
    pub fn execute_transactions(&mut self) -> Result<()> {
        let total_difficulty = U256::ZERO;
        let chain_spec = ChainSpecBuilder::default()
            .chain(MAINNET.chain)
            .genesis(MAINNET.genesis.clone())
            .cancun_activated()
            .build();

        let mut db: MemDb = create_db(&mut self.input).unwrap();

        let executor =
            EthExecutorProvider::ethereum(chain_spec.clone().into()).eth_executor(db.clone());
        let EthBlockOutput { state, receipts, gas_used, db: full_state } = executor.execute(
            (
                &self
                .input
                .block
                    .clone()
                    .with_recovered_senders()
                    .ok_or(BlockValidationError::SenderRecoveryError).expect("brecht"),
                total_difficulty.into(),
            )
                .into(),
        ).expect("brecht");

        db.commit_from_bundle(state);

        self.db = db;

        Ok(())
    }

    /// Finalizes the block building and returns the header and the state trie.
    pub fn finalize(&mut self) -> Result<AlloyConsensusHeader> {
        // apply state updates
        let mut state_trie = mem::take(&mut self.input.parent_state_trie);
        for (address, account) in &self.db.accounts {
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

        // update result header with the new state root
        let mut header = self.header.take().expect("Header not initialized");
        header.state_root = state_trie.hash();

        // Leak memory, save cycles
        //guest_mem_forget(block_builder);

        Ok(header)
    }
}