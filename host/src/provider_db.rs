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
use std::{collections::HashSet, mem::take};

use alloy_consensus::Header as AlloyConsensusHeader;
use alloy_primitives::Bytes;
use raiko_lib::{builder::OptimisticDatabase, consts::ChainSpec, mem_db::MemDb, utils::to_header};
use raiko_primitives::{Address, B256, U256};
use revm::{
    primitives::{Account, AccountInfo, Bytecode, HashMap},
    Database, DatabaseCommit,
};
use tokio::runtime::Handle;

use crate::{raiko::BlockDataProvider, MerkleProof};

pub struct ProviderDb<BDP: BlockDataProvider> {
    pub provider: BDP,
    pub block_number: u64,
    pub initial_db: MemDb,
    pub initial_headers: HashMap<u64, AlloyConsensusHeader>,
    pub current_db: MemDb,
    async_executor: Handle,

    pub optimistic: bool,
    pub staging_db: MemDb,
    pub pending_accounts: HashSet<Address>,
    pub pending_slots: HashSet<(Address, U256)>,
    pub pending_block_hashes: HashSet<u64>,
}

impl<BDP: BlockDataProvider> ProviderDb<BDP> {
    pub async fn new(
        provider: BDP,
        chain_spec: ChainSpec,
        block_number: u64,
    ) -> Result<Self, anyhow::Error> {
        let mut provider_db = ProviderDb {
            provider,
            block_number,
            initial_db: Default::default(),
            initial_headers: Default::default(),
            current_db: Default::default(),
            async_executor: tokio::runtime::Handle::current(),
            optimistic: false,
            staging_db: Default::default(),
            pending_accounts: HashSet::new(),
            pending_slots: HashSet::new(),
            pending_block_hashes: HashSet::new(),
        };
        if chain_spec.is_taiko() {
            // Get the 256 history block hashes from the provider at first time for anchor
            // transaction.
            let start = block_number.saturating_sub(255);
            let block_numbers = (start..=block_number)
                .map(|block_number| (block_number, false))
                .collect::<Vec<_>>();
            let initial_history_blocks = provider_db.provider.get_blocks(&block_numbers).await?;
            for block in initial_history_blocks {
                let block_number: u64 = block.header.number.unwrap().try_into().unwrap();
                let block_hash = block.header.hash.unwrap();
                provider_db
                    .initial_db
                    .insert_block_hash(block_number, block_hash);
                provider_db
                    .initial_headers
                    .insert(block_number, to_header(&block.header));
            }
        }
        Ok(provider_db)
    }

    pub async fn get_proofs(&mut self) -> Result<(MerkleProof, MerkleProof, usize), anyhow::Error> {
        // Latest proof keys
        let mut storage_keys = self.initial_db.storage_keys();
        for (address, mut indices) in self.current_db.storage_keys() {
            match storage_keys.get_mut(&address) {
                Some(initial_indices) => initial_indices.append(&mut indices),
                None => {
                    storage_keys.insert(address, indices);
                }
            }
        }

        // Calculate how many storage proofs we need
        let num_initial_values: usize = self
            .initial_db
            .storage_keys()
            .values()
            .map(|keys| keys.len())
            .sum();
        let num_latest_values: usize = storage_keys.values().map(|keys| keys.len()).sum();
        let num_storage_proofs = num_initial_values + num_latest_values;

        // Initial proofs
        let initial_proofs = self
            .provider
            .get_merkle_proofs(
                self.block_number,
                self.initial_db.storage_keys(),
                0,
                num_storage_proofs,
            )
            .await?;
        let latest_proofs = self
            .provider
            .get_merkle_proofs(
                self.block_number + 1,
                storage_keys,
                num_initial_values,
                num_storage_proofs,
            )
            .await?;

        Ok((initial_proofs, latest_proofs, num_storage_proofs))
    }

    pub async fn get_ancestor_headers(
        &mut self,
    ) -> Result<Vec<AlloyConsensusHeader>, anyhow::Error> {
        let earliest_block = self
            .initial_db
            .block_hashes
            .keys()
            .min()
            .unwrap_or(&self.block_number);

        let mut headers = Vec::new();
        for block_number in (*earliest_block..self.block_number).rev() {
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.initial_headers.entry(block_number)
            {
                let block = &self.provider.get_blocks(&[(block_number, false)]).await?[0];
                e.insert(to_header(&block.header));
            }
            headers.push(self.initial_headers[&block_number].clone());
        }
        Ok(headers)
    }

    pub fn is_valid_run(&self) -> bool {
        self.pending_accounts.is_empty()
            && self.pending_slots.is_empty()
            && self.pending_block_hashes.is_empty()
    }
}

impl<BDP: BlockDataProvider> Database for ProviderDb<BDP> {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Check if the account is in the current database.
        if let Ok(db_result) = self.current_db.basic(address) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.initial_db.basic(address) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.staging_db.basic(address) {
            if self.is_valid_run() {
                self.initial_db
                    .insert_account_info(address, db_result.clone().unwrap());
            }
            return Ok(db_result);
        }

        // In optimistic mode, don't wait on the data and just return some default values
        if self.optimistic {
            self.pending_accounts.insert(address);

            let code = Bytes::from(vec![]);
            let account_info = AccountInfo::new(
                U256::ZERO,
                u64::MAX,
                Bytecode::new_raw(code.clone()).hash_slow(),
                Bytecode::new_raw(code),
            );
            return Ok(Some(account_info));
        }

        // Fetch the account
        let account = &tokio::task::block_in_place(|| {
            self.async_executor
                .block_on(self.provider.get_accounts(&[address]))
        })?[0];

        // Insert the account into the initial database.
        self.initial_db
            .insert_account_info(address, account.clone());
        Ok(Some(account.clone()))
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        // Check if the storage slot is in the current database.
        if let Ok(db_result) = self.current_db.storage(address, index) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.initial_db.storage(address, index) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.staging_db.storage(address, index) {
            if self.is_valid_run() {
                self.initial_db
                    .insert_account_storage(&address, index, db_result);
            }
            return Ok(db_result);
        }

        // In optimistic mode, don't wait on the data and just return a default value
        if self.optimistic {
            self.basic(address)?;
            self.pending_slots.insert((address, index));
            return Ok(U256::default());
        }

        // Makes sure the account is also always loaded
        self.initial_db.basic(address)?;

        // Fetch the storage value
        let value = tokio::task::block_in_place(|| {
            self.async_executor
                .block_on(self.provider.get_storage_values(&[(address, index)]))
        })?[0];
        self.initial_db
            .insert_account_storage(&address, index, value);
        Ok(value)
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        let block_number = u64::try_from(number).unwrap();

        // Check if the block hash is in the current database.
        if let Ok(block_hash) = self.initial_db.block_hash(number) {
            return Ok(block_hash);
        }
        if let Ok(db_result) = self.staging_db.block_hash(number) {
            if self.is_valid_run() {
                self.initial_db.insert_block_hash(block_number, db_result);
            }
            return Ok(db_result);
        }

        // In optimistic mode, don't wait on the data and just return some default values
        if self.optimistic {
            self.pending_block_hashes.insert(block_number);
            return Ok(B256::default());
        }

        // Get the block hash from the provider.
        let block_hash = tokio::task::block_in_place(|| {
            self.async_executor
                .block_on(self.provider.get_blocks(&[(block_number, false)]))
        })
        .unwrap()[0]
            .header
            .hash
            .unwrap()
            .0
            .into();
        self.initial_db.insert_block_hash(block_number, block_hash);
        Ok(block_hash)
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        unreachable!()
    }
}

impl<BDP: BlockDataProvider> DatabaseCommit for ProviderDb<BDP> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.current_db.commit(changes)
    }
}

impl<BDP: BlockDataProvider> OptimisticDatabase for ProviderDb<BDP> {
    async fn fetch_data(&mut self) -> bool {
        //println!("all accounts touched: {:?}", self.pending_accounts);
        //println!("all slots touched: {:?}", self.pending_slots);
        //println!("all block hashes touched: {:?}", self.pending_block_hashes);

        // This run was valid when no pending work was scheduled
        let valid_run = self.is_valid_run();

        let accounts = self
            .provider
            .get_accounts(&self.pending_accounts.iter().cloned().collect::<Vec<_>>())
            .await
            .unwrap();
        for (address, account) in take(&mut self.pending_accounts)
            .into_iter()
            .zip(accounts.iter())
        {
            self.staging_db
                .insert_account_info(address, account.clone());
        }

        let slots = self
            .provider
            .get_storage_values(&self.pending_slots.iter().cloned().collect::<Vec<_>>())
            .await
            .unwrap();
        for ((address, index), value) in take(&mut self.pending_slots).into_iter().zip(slots.iter())
        {
            self.staging_db
                .insert_account_storage(&address, index, *value);
        }

        let blocks = self
            .provider
            .get_blocks(
                &self
                    .pending_block_hashes
                    .iter()
                    .cloned()
                    .map(|block_number| (block_number, false))
                    .collect::<Vec<_>>(),
            )
            .await
            .unwrap();
        for (block_number, block) in take(&mut self.pending_block_hashes)
            .into_iter()
            .zip(blocks.iter())
        {
            self.staging_db
                .insert_block_hash(block_number, block.header.hash.unwrap());
            self.initial_headers
                .insert(block_number, to_header(&block.header));
        }

        // If this wasn't a valid run, clear the post execution database
        if !valid_run {
            self.current_db = Default::default();
        }

        valid_run
    }

    fn is_optimistic(&self) -> bool {
        self.optimistic
    }
}
