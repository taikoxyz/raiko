use core::mem;
use std::sync::Arc;

use crate::primitives::keccak::keccak;
use crate::primitives::mpt::StateAccount;
use crate::utils::generate_transactions;
use crate::{
    consts::{ChainSpec, MAX_BLOCK_HASH_AGE},
    guest_mem_forget,
    input::GuestInput,
    mem_db::{AccountState, DbAccount, MemDb},
};
use alloy_consensus::{Signed, Transaction, TxEnvelope};
use alloy_rpc_types::{ConversionError, Parity, Transaction as AlloyTransaction};
use anyhow::{bail, Context, Error, Result};
use reth_evm::execute::EthBlockOutput;
use reth_evm::execute::Executor;
use reth_evm_ethereum::execute::EthExecutorProvider;
use reth_evm_ethereum::taiko::TaikoData;
use reth_interfaces::executor::BlockValidationError;
use reth_primitives::revm_primitives::db::{Database, DatabaseCommit};
use reth_primitives::revm_primitives::{AccountInfo, Bytecode, Bytes, HashMap, SpecId};
use reth_primitives::transaction::Signature as RethSignature;
use reth_primitives::{
    BlockBody, ChainSpecBuilder, Header, TransactionSigned, B256, HOLESKY, KECCAK_EMPTY, MAINNET,
    TAIKO_A7, TAIKO_MAINNET, U256,
};
use reth_provider::ProviderError;
use reth_revm::revm::db::BundleState;

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

/// Minimum supported protocol version: SHANGHAI
const MIN_SPEC_ID: SpecId = SpecId::SHANGHAI;

impl<DB: Database<Error = ProviderError> + DatabaseCommit + OptimisticDatabase>
    RethBlockBuilder<DB>
{
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
        let chain_spec = &self.input.chain_spec;
        let is_taiko = chain_spec.is_taiko();

        let total_difficulty = U256::ZERO;
        let reth_chain_spec = match chain_spec.name.as_str() {
            "taiko_a7" => TAIKO_A7.clone(),
            "taiko_mainnet" => TAIKO_MAINNET.clone(),
            "ethereum" => {
                //MAINNET.clone()
                // TODO(Brecht): for some reason using the spec directly doesn't work
                Arc::new(
                    ChainSpecBuilder::default()
                        .chain(MAINNET.chain)
                        .genesis(MAINNET.genesis.clone())
                        .cancun_activated()
                        .build(),
                )
            }
            "holesky" => HOLESKY.clone(),
            _ => unimplemented!(),
        };

        let header = self.header.as_mut().expect("Header is not initialized");
        let spec_id = self
            .input
            .chain_spec
            .active_fork(header.number, header.timestamp)
            .unwrap();
        if !SpecId::enabled(spec_id, MIN_SPEC_ID) {
            bail!("Invalid protocol version: expected >= {MIN_SPEC_ID:?}, got {spec_id:?}")
        }

        // generate the transactions from the tx list
        // For taiko blocks, insert the anchor tx as the first transaction
        let anchor_tx = if is_taiko {
            Some(serde_json::from_str(&self.input.taiko.anchor_tx.clone()).unwrap())
        } else {
            None
        };
        let transactions = generate_transactions(
            &self.input.chain_spec,
            self.input.taiko.block_proposed.meta.blobUsed,
            &self.input.taiko.tx_data,
            anchor_tx,
        );
        let mut alloy_transactions = Vec::new();
        for tx in transactions {
            let alloy_tx: AlloyTransaction =
                to_alloy_transaction(&tx).expect("can't convert to alloy");
            alloy_transactions.push(alloy_tx);
        }

        let mut block = self.input.block.clone();
        // Convert alloy transactions to reth transactions and set them on the block
        block.body = alloy_transactions
            .into_iter()
            .map(|tx| {
                let signature = tx
                    .signature
                    .ok_or(ConversionError::MissingSignature)
                    .expect("missing signature");
                TransactionSigned::from_transaction_and_signature(
                    tx.try_into().expect("invalid signature"),
                    RethSignature {
                        r: signature.r,
                        s: signature.s,
                        odd_y_parity: signature
                            .y_parity
                            .unwrap_or_else(|| reth_rpc_types::Parity(!signature.v.bit(0)))
                            .0,
                    },
                )
            })
            .collect();

        let executor = EthExecutorProvider::ethereum(reth_chain_spec.clone().into())
            .eth_executor(self.db.take().unwrap())
            .taiko_data(TaikoData {
                l1_header: self.input.taiko.l1_header.clone(),
                parent_header: self.input.parent_header.clone(),
                l2_contract: self.input.chain_spec.l2_contract.unwrap_or_default(),
            })
            .optimistic(optimistic);
        let EthBlockOutput {
            state,
            receipts: _,
            gas_used,
            db: full_state,
        } = executor.execute(
            (
                &block
                    .clone()
                    .with_recovered_senders()
                    .ok_or(BlockValidationError::SenderRecoveryError)?,
                total_difficulty.into(),
            )
                .into(),
        )?;

        self.db = Some(full_state.database);

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
            let value: U256 = storage_trie
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

pub fn to_alloy_transaction(tx: &TxEnvelope) -> Result<AlloyTransaction, Error> {
    match tx {
        TxEnvelope::Legacy(tx) => {
            let alloy_tx = AlloyTransaction {
                hash: *tx.hash(),
                nonce: tx.tx().nonce(),
                block_hash: None,
                block_number: None,
                transaction_index: None,
                to: tx.tx().to().to().copied(),
                value: tx.tx().value(),
                gas_price: tx.tx().gas_price(),
                gas: tx.tx().gas_limit(),
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                max_fee_per_blob_gas: None,
                input: tx.tx().input().to_owned().into(),
                signature: Some(to_alloy_signature(get_sig(tx))),
                chain_id: tx.tx().chain_id(),
                blob_versioned_hashes: None,
                access_list: None,
                transaction_type: Some(0),
                ..Default::default()
            };
            Ok(alloy_tx)
        }
        TxEnvelope::Eip2930(tx) => {
            let alloy_tx = AlloyTransaction {
                hash: *tx.hash(),
                nonce: tx.tx().nonce(),
                block_hash: None,
                block_number: None,
                transaction_index: None,
                to: tx.tx().to().to().copied(),
                value: tx.tx().value(),
                gas_price: tx.tx().gas_price(),
                gas: tx.tx().gas_limit(),
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                max_fee_per_blob_gas: None,
                input: tx.tx().input().to_owned().into(),
                signature: Some(to_alloy_signature(get_sig(tx))),
                chain_id: tx.tx().chain_id(),
                blob_versioned_hashes: None,
                access_list: None,
                transaction_type: Some(1),
                ..Default::default()
            };
            Ok(alloy_tx)
        }
        TxEnvelope::Eip1559(tx) => {
            let alloy_tx = AlloyTransaction {
                hash: *tx.hash(),
                nonce: tx.tx().nonce(),
                block_hash: None,
                block_number: None,
                transaction_index: None,
                to: tx.tx().to().to().copied(),
                value: tx.tx().value(),
                gas_price: tx.tx().gas_price(),
                gas: tx.tx().gas_limit(),
                max_fee_per_gas: Some(tx.tx().max_fee_per_gas),
                max_priority_fee_per_gas: Some(tx.tx().max_priority_fee_per_gas),
                max_fee_per_blob_gas: None,
                input: tx.tx().input().to_owned().into(),
                signature: Some(to_alloy_signature(get_sig(tx))),
                chain_id: tx.tx().chain_id(),
                blob_versioned_hashes: None,
                access_list: Some(tx.tx().access_list.clone()),
                transaction_type: Some(2),
                ..Default::default()
            };
            Ok(alloy_tx)
        }
        TxEnvelope::Eip4844(tx) => {
            let alloy_tx = AlloyTransaction {
                hash: *tx.hash(),
                nonce: tx.tx().nonce(),
                block_hash: None,
                block_number: None,
                transaction_index: None,
                to: tx.tx().to().to().copied(),
                value: tx.tx().value(),
                gas_price: tx.tx().gas_price(),
                gas: tx.tx().gas_limit(),
                max_fee_per_gas: Some(tx.tx().tx().max_fee_per_gas),
                max_priority_fee_per_gas: Some(tx.tx().tx().max_priority_fee_per_gas),
                max_fee_per_blob_gas: Some(tx.tx().tx().max_fee_per_blob_gas),
                input: tx.tx().input().to_owned().into(),
                signature: Some(to_alloy_signature(get_sig(tx))),
                chain_id: tx.tx().chain_id(),
                blob_versioned_hashes: Some(tx.tx().tx().blob_versioned_hashes.clone()),
                access_list: Some(tx.tx().tx().access_list.clone()),
                transaction_type: Some(tx.tx().tx_type() as u8),
                ..Default::default()
            };
            Ok(alloy_tx)
        }
        _ => todo!(),
    }
}

pub fn get_sig<T, Sig: Clone>(tx: &Signed<T, Sig>) -> Sig {
    tx.signature().clone()
}

pub fn to_alloy_signature(sig: alloy_primitives::Signature) -> alloy_rpc_types::Signature {
    alloy_rpc_types::Signature {
        r: sig.r(),
        s: sig.s(),
        v: sig.v().to_parity_bool().y_parity_byte().try_into().unwrap(),
        y_parity: Some(Parity(sig.v().to_parity_bool().y_parity())),
    }
}
