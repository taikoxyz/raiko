use std::collections::{hash_map::Entry, HashMap};

use reth_evm::execute::ProviderError;
use reth_primitives::revm_primitives::{
    db::{Database, DatabaseCommit},
    Account, AccountInfo, Bytecode,
};
use serde::{Deserialize, Serialize};
use thiserror_no_std::Error as ThisError;

#[cfg(not(feature = "std"))]
use crate::no_std::*;

use crate::{
    builder::OptimisticDatabase,
    primitives::{Address, B256, U256},
};

/// Error returned by the [MemDb].
#[derive(Debug, ThisError)]
pub enum DbError {
    /// Returned when an account was accessed but not loaded into the DB.
    #[error("account {0} not loaded")]
    AccountNotFound(Address),
    /// Returned when account info was different before inserting.
    #[error("account info {0} different")]
    AccountInfoMissmatch(Address),
    /// Returned when storage was accessed but not loaded into the DB.
    #[error("storage {1}@{0} not loaded")]
    SlotNotFound(Address, U256),
    /// Returned when a block hash was accessed but not loaded into the DB.
    #[error("block {0} not loaded")]
    BlockNotFound(u64),
    /// Returned when blockhash was different before inserting.
    #[error("blockhash {0} different")]
    BlockHashMissmatch(u64),
    /// Unspecified error.
    #[error(transparent)]
    Unspecified(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountState {
    // Account can be cleared/removed from state.
    Deleted,
    /// EVM touched this account.
    Touched,
    /// EVM cleared storage of this account, mostly by `selfdestruct`.
    StorageCleared,
    /// EVM didn't interacted with this account.
    #[default]
    None,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DbAccount {
    pub info: AccountInfo,
    pub state: AccountState,
    pub storage: HashMap<U256, U256>,
}

impl DbAccount {
    pub fn new(info: AccountInfo) -> Self {
        Self {
            info,
            ..Self::default()
        }
    }

    /// Return the account info or `None` if the account has been deleted.
    pub fn info(&self) -> Option<AccountInfo> {
        if self.state == AccountState::Deleted {
            None
        } else {
            Some(self.info.clone())
        }
    }
}

/// In-memory EVM database.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemDb {
    /// Account info where None means it is not existing.
    pub accounts: HashMap<Address, DbAccount>,
    /// All cached block hashes.
    pub block_hashes: HashMap<u64, B256>,
}

impl MemDb {
    pub fn accounts_len(&self) -> usize {
        self.accounts.len()
    }

    pub fn storage_keys(&self) -> HashMap<Address, Vec<U256>> {
        self.accounts
            .iter()
            .map(|(address, account)| (*address, account.storage.keys().copied().collect()))
            .collect()
    }

    /// Insert account info without overriding its storage.
    /// Panics if a different account info exists.
    pub fn insert_account_info(
        &mut self,
        address: Address,
        info: AccountInfo,
    ) -> Result<(), DbError> {
        match self.accounts.entry(address) {
            Entry::Occupied(entry) => {
                if info != entry.get().info {
                    return Err(DbError::AccountInfoMissmatch(address));
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(DbAccount::new(info));
            }
        }
        Ok(())
    }

    /// insert account storage without overriding the account info.
    /// Panics if the account does not exist.
    pub fn insert_account_storage(
        &mut self,
        address: &Address,
        index: U256,
        data: U256,
    ) -> Result<(), DbError> {
        let Some(account) = self.accounts.get_mut(address) else {
            return Err(DbError::AccountNotFound(*address));
        };
        account.storage.insert(index, data);
        Ok(())
    }

    /// Insert the specified block hash. Panics if a different block hash exists.
    pub fn insert_block_hash(&mut self, block_no: u64, block_hash: B256) -> Result<(), DbError> {
        match self.block_hashes.entry(block_no) {
            Entry::Occupied(entry) => {
                if &block_hash != entry.get() {
                    return Err(DbError::BlockHashMissmatch(block_no));
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(block_hash);
            }
        };
        Ok(())
    }
}

impl Database for MemDb {
    type Error = ProviderError;

    /// Get basic account information.
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.accounts.get(&address).map_or_else(
            || Err(ProviderError::BestBlockNotFound),
            |db_account| Ok(db_account.info()),
        )
    }

    /// Get account code by its hash.
    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        // not needed because we already load code with basic info
        unreachable!()
    }

    /// Get storage value of address at index.
    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.accounts.get(&address).map_or_else(
            // otherwise this is an uncached load
            || Err(ProviderError::BestBlockNotFound),
            // if we have this account in the cache, we can query its storage
            |account| {
                if let Some(value) = account.storage.get(&index).copied() {
                    Ok(value)
                } else {
                    match account.state {
                        // it is impossible to access the storage from a non-existing account
                        AccountState::Deleted => unreachable!(),
                        // if the account has been deleted or cleared, we must return 0
                        AccountState::StorageCleared => Ok(U256::ZERO),
                        // otherwise this is an uncached load
                        _ => Err(ProviderError::BestBlockNotFound),
                    }
                }
            },
        )
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        let block_no =
            u64::try_from(number).map_err(|_| ProviderError::BlockNumberOverflow(number))?;
        self.block_hashes
            .get(&block_no)
            .copied()
            .ok_or(ProviderError::BestBlockNotFound)
    }
}

impl DatabaseCommit for MemDb {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        for (address, new_account) in changes {
            // if nothing was touched, there is nothing to do
            if !new_account.is_touched() {
                continue;
            }

            if new_account.is_selfdestructed() {
                // get the account we are destroying
                let Some(db_account) = self.accounts.get_mut(&address) else {
                    // destruction of a non-existing account, so there is nothing to do
                    // a) the account was created and destroyed in the same transaction
                    // b) or it was destroyed without reading and thus not cached
                    continue;
                };

                // it is not possible to delete a deleted account
                debug_assert!(db_account.state != AccountState::Deleted);

                // clear the account and mark it as deleted
                db_account.storage.clear();
                db_account.state = AccountState::Deleted;
                db_account.info = AccountInfo::default();

                continue;
            }

            // empty accounts cannot have any non-zero storage
            if new_account.is_empty() {
                debug_assert!(new_account.storage.is_empty());
            }

            let is_newly_created = new_account.is_created();

            // update account info
            let db_account = match self.accounts.entry(address) {
                Entry::Occupied(entry) => {
                    let db_account = entry.into_mut();

                    // the account was touched, but it is now empty, so it should be deleted
                    // this also deletes empty accounts previously contained in the state trie
                    if new_account.is_empty() {
                        // if the account is empty, it must be deleted
                        db_account.storage.clear();
                        db_account.state = AccountState::Deleted;
                        db_account.info = AccountInfo::default();

                        continue;
                    }

                    // update the account info
                    db_account.info = new_account.info;
                    db_account
                }
                Entry::Vacant(entry) => {
                    // create a new account only if it is not empty
                    if new_account.is_empty() {
                        continue;
                    }

                    // create new non-empty account
                    entry.insert(DbAccount::new(new_account.info))
                }
            };

            // set the correct state
            db_account.state = if is_newly_created {
                db_account.storage.clear();
                AccountState::StorageCleared
            } else if db_account.state == AccountState::StorageCleared {
                // when creating the storage trie, it must be cleared it first
                AccountState::StorageCleared
            } else {
                AccountState::Touched
            };

            // update all changed storage values
            db_account.storage.extend(
                new_account
                    .storage
                    .into_iter()
                    .filter(|(_, value)| value.is_changed())
                    .map(|(key, value)| (key, value.present_value())),
            );
        }
    }
}

impl OptimisticDatabase for MemDb {
    async fn fetch_data(&mut self) -> bool {
        true
    }

    fn is_optimistic(&self) -> bool {
        false
    }
}
