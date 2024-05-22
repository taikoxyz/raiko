use core::mem;

use alloy_primitives::uint;
use anyhow::{bail, Context, Error, Result};
use raiko_primitives::{keccak::keccak, mpt::{MptNode, StateAccount}, RlpBytes};
use reth_evm::execute::EthBlockOutput;
use reth_evm_ethereum::execute::EthExecutorProvider;
use reth_interfaces::executor::BlockValidationError;
use reth_primitives::{BlockBody, ChainSpecBuilder, Header, Receipts, B256, MAINNET, U256};
use reth_provider::{BundleStateWithReceipts, OriginalValuesKnown, ProviderError};
use revm::{db::BundleState, Database, DatabaseCommit};
use reth_evm::execute::Executor;
use raiko_primitives::{
    keccak::{KECCAK_EMPTY},
    Bytes,
};
use revm::primitives::{AccountInfo, Bytecode, HashMap};
use crate::{
    consts::{MAX_BLOCK_HASH_AGE, ChainSpec}, guest_mem_forget, input::GuestInput, mem_db::{AccountState, DbAccount, MemDb},
};

/// Optimistic database
#[allow(async_fn_in_trait)]
pub trait OptimisticDatabase {
    /// Handle post execution work
    async fn fetch_data(&mut self) -> bool;

    /// Commit changes to the database.
    fn commit_from_bundle(&mut self, bundle: BundleState);

    /// If the current database is optimistic
    fn is_optimistic(&self) -> bool;
}
/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct RethBlockBuilder<DB> {
    pub chain_spec: ChainSpec,
    pub input: GuestInput,
    pub db: Option<DB>,
    pub header: Option<Header>,
}

impl<DB: Database<Error = ProviderError> + DatabaseCommit + OptimisticDatabase> RethBlockBuilder<DB> {
    /// Creates a new block builder.
    pub fn new(input: &GuestInput, db: DB) -> RethBlockBuilder<DB> {
        RethBlockBuilder {
            chain_spec: input.chain_spec.clone(),
            db: Some(db),
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
        self.header = Some(Header {
            // Initialize fields that we can compute from the parent
            parent_hash: self.input.parent_header.hash_slow(),
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
            // do not fill the remaining fields
            ..Default::default()
        });
        Ok(())
    }

    /// Executes all input transactions.
    pub fn execute_transactions(&mut self, optimistic: bool) -> Result<()> {
        let total_difficulty = U256::ZERO;
        let chain_spec = ChainSpecBuilder::default()
            .chain(MAINNET.chain)
            .genesis(MAINNET.genesis.clone())
            .cancun_activated()
            .build();

        let executor =
            EthExecutorProvider::ethereum(chain_spec.clone().into()).eth_executor(self.db.take().unwrap()).optimistic(optimistic);
        let EthBlockOutput { state, receipts: _, gas_used, db: full_state } = executor.execute(
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

        self.db = Some(full_state.database);

        /*let receipts = receipts.iter().map(|r| Some(r.clone())).collect::<Vec<_>>();
        let bundle_state = BundleStateWithReceipts::new(
            state.clone(),
            Receipts { receipt_vec: vec![receipts] },
            self.input.block_number,
        );

        let state = executor.take_output_state();

        executor.execute_and_verify_receipt(&block_with_senders.clone().unseal(), U256::MAX)?;
                let state = executor.take_output_state();
                debug!(target: "reth::cli", ?state, "Executed block");*/

        /*let hashed_state = bundle_state.hash_state_slow();
        let (state_root, trie_updates) = bundle_state
            .hash_state_slow()
            .state_root_with_updates(provider_factory.provider()?.tx_ref())?;*/

        self.db.as_mut().unwrap().commit_from_bundle(state);

        // Set the values verified in reth in execute
        let header = self.header.as_mut().unwrap();
        header.gas_used = gas_used.into();
        header.receipts_root = self.input.block.header.receipts_root;
        header.logs_bloom = self.input.block.header.logs_bloom;

        Ok(())
    }
}

impl RethBlockBuilder<MemDb> {
    /// Finalizes the block building and returns the header and the state trie.
    pub fn finalize(&mut self) -> Result<Header> {
        let mut header = self.header.take().expect("Header not initialized");
        let block_body = BlockBody::from(self.input.block.clone());

        header.state_root = self.calculate_state_root()?;
        header.transactions_root = block_body.calculate_tx_root();
        header.withdrawals_root = block_body.calculate_withdrawals_root();
        header.ommers_hash = block_body.calculate_ommers_root();

        Ok(header)
    }

    /// Finalizes the block building and returns the header and the state trie.
    pub fn calculate_state_root(&mut self) -> Result<B256> {
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
        Ok(state_trie.hash())
    }
}

pub fn create_mem_db(input: &mut GuestInput) -> Result<MemDb> {
    // Verify state trie root
    if input.parent_state_trie.hash()
        != input.parent_header.state_root
    {
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

    // Load account data into db
    let mut accounts = HashMap::with_capacity(input.parent_storage.len());
    for (address, (storage_trie, slots)) in &mut input.parent_storage {
        // consume the slots, as they are no longer needed afterwards
        let slots = mem::take(slots);

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
            let bytes = contracts
                .get(&code_hash)
                .expect("Contract not found")
                .clone();
            Bytecode::new_raw(bytes)
        };

        // load storage reads
        let mut storage = HashMap::with_capacity(slots.len());
        for slot in slots {
            let value: raiko_primitives::U256 = storage_trie
                .get_rlp(&keccak(slot.to_be_bytes::<32>()))?
                .unwrap_or_default();
            storage.insert(slot, value);
        }

        let mem_account = DbAccount {
            info: AccountInfo {
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

    // prepare block hash history
    let mut block_hashes =
        HashMap::with_capacity(input.ancestor_headers.len() + 1);
    block_hashes.insert(
        input.parent_header.number,
        input.parent_header.hash_slow(),
    );
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