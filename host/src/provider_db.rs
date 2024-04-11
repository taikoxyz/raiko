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
use std::{
    ops::AddAssign,
    time::{Duration, Instant},
};

use alloy_consensus::Header as AlloyConsensusHeader;
use alloy_provider::{Provider, ReqwestProvider};
use alloy_rpc_types::{Block, BlockId, EIP1186AccountProofResponse};
use raiko_lib::{clear_line, inplace_print, mem_db::MemDb, print_duration, taiko_utils::to_header};
use raiko_primitives::{Address, B256, U256};
use revm::{
    primitives::{Account, AccountInfo, Bytecode, HashMap},
    Database, DatabaseCommit,
};
use tokio::runtime::Handle;

use crate::preflight::get_block;

pub struct ProviderDb {
    pub provider: ReqwestProvider,
    pub block_number: u64,
    pub initial_db: MemDb,
    pub initial_headers: HashMap<u64, AlloyConsensusHeader>,
    pub current_db: MemDb,
    async_executor: Handle,
}

impl ProviderDb {
    pub fn new(
        provider: ReqwestProvider,
        initial_history_blocks: Option<Vec<Block>>,
        block_number: u64,
    ) -> Self {
        let mut initial_db = MemDb::default();
        let mut initial_headers = HashMap::new();
        if let Some(initial_history_blocks) = initial_history_blocks {
            for block in initial_history_blocks {
                let block_number: u64 = block.header.number.unwrap().try_into().unwrap();
                let block_hash = block.header.hash.unwrap();
                initial_db.insert_block_hash(block_number, block_hash);
                initial_headers.insert(block_number, to_header(&block.header));
            }
        }
        ProviderDb {
            provider,
            block_number,
            initial_db,
            initial_headers,
            current_db: MemDb::default(),
            async_executor: tokio::runtime::Handle::current(),
        }
    }

    pub fn get_initial_db(&self) -> &MemDb {
        &self.initial_db
    }

    pub fn get_latest_db(&self) -> &MemDb {
        &self.current_db
    }

    fn get_storage_proofs(
        &mut self,
        block_number: u64,
        storage_keys: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
        let mut storage_proofs = HashMap::new();
        let mut idx = offset;
        for (address, keys) in storage_keys {
            inplace_print(&format!(
                "fetching storage proof {idx}/{num_storage_proofs}..."
            ));

            let indices = keys.iter().map(|x| x.to_be_bytes().into()).collect();
            let proof = self.async_executor.block_on(async {
                self.provider
                    .get_proof(address, indices, Some(BlockId::from(block_number)))
                    .await
            })?;
            storage_proofs.insert(address, proof);
            idx += keys.len();
        }
        clear_line();

        Ok(storage_proofs)
    }

    pub fn get_proofs(
        &mut self,
    ) -> Result<
        (
            HashMap<Address, EIP1186AccountProofResponse>,
            HashMap<Address, EIP1186AccountProofResponse>,
            usize,
        ),
        anyhow::Error,
    > {
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
            .iter()
            .map(|(_address, keys)| keys.len())
            .sum();
        let num_latest_values: usize = storage_keys.iter().map(|(_address, keys)| keys.len()).sum();
        let num_storage_proofs = num_initial_values + num_latest_values;

        // Initial proofs
        let initial_proofs = self.get_storage_proofs(
            self.block_number,
            self.initial_db.storage_keys(),
            0,
            num_storage_proofs,
        )?;
        let latest_proofs = self.get_storage_proofs(
            self.block_number + 1,
            storage_keys,
            num_initial_values,
            num_storage_proofs,
        )?;

        Ok((initial_proofs, latest_proofs, num_storage_proofs))
    }

    pub fn get_ancestor_headers(&mut self) -> Result<Vec<AlloyConsensusHeader>, anyhow::Error> {
        let earliest_block = self
            .initial_db
            .block_hashes
            .keys()
            .min()
            .unwrap_or(&self.block_number);
        let headers = (*earliest_block..self.block_number)
            .rev()
            .map(|block_number| {
                self.initial_headers
                    .get(&block_number)
                    .cloned()
                    .unwrap_or_else(|| {
                        to_header(
                            &get_block(&self.provider, block_number, false)
                                .unwrap()
                                .header,
                        )
                    })
            })
            .collect();
        Ok(headers)
    }
}

impl Database for ProviderDb {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Check if the account is in the current database.
        if let Ok(db_result) = self.current_db.basic(address) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.initial_db.basic(address) {
            return Ok(db_result);
        }

        // Get the nonce, balance, and code to reconstruct the account.
        let nonce = self.async_executor.block_on(async {
            self.provider
                .get_transaction_count(address, Some(BlockId::from(self.block_number)))
                .await
        })?;
        let balance = self.async_executor.block_on(async {
            self.provider
                .get_balance(address, Some(BlockId::from(self.block_number)))
                .await
        })?;
        let code = self.async_executor.block_on(async {
            self.provider
                .get_code_at(address, BlockId::from(self.block_number))
                .await
        })?;

        // Insert the account into the initial database.
        let account_info = AccountInfo::new(
            balance,
            nonce.try_into().unwrap(),
            Bytecode::new_raw(code.clone()).hash_slow(),
            Bytecode::new_raw(code),
        );
        self.initial_db
            .insert_account_info(address, account_info.clone());
        Ok(Some(account_info))
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        // Check if the storage slot is in the current database.
        if let Ok(db_result) = self.current_db.storage(address, index) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.initial_db.storage(address, index) {
            return Ok(db_result);
        }

        // Get the storage slot from the provider.
        self.initial_db.basic(address)?;
        let storage = self.async_executor.block_on(async {
            self.provider
                .get_storage_at(
                    address.into_array().into(),
                    index,
                    Some(BlockId::from(self.block_number)),
                )
                .await
        })?;
        self.initial_db
            .insert_account_storage(&address, index, storage);
        Ok(storage)
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        // Check if the block hash is in the current database.
        if let Ok(block_hash) = self.initial_db.block_hash(number) {
            return Ok(block_hash);
        }

        let block_number = u64::try_from(number).unwrap();
        // Get the block hash from the provider.
        let block_hash = self.async_executor.block_on(async {
            self.provider
                .get_block_by_number(block_number.into(), false)
                .await
                .unwrap()
                .unwrap()
                .header
                .hash
                .unwrap()
                .0
                .into()
        });
        self.initial_db.insert_block_hash(block_number, block_hash);
        Ok(block_hash)
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        unreachable!()
    }
}

impl DatabaseCommit for ProviderDb {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.current_db.commit(changes)
    }
}

pub struct MeasuredProviderDb {
    pub provider: ProviderDb,
    pub num_basic: u64,
    pub time_basic: Duration,
    pub num_storage: u64,
    pub time_storage: Duration,
    pub num_block_hash: u64,
    pub time_block_hash: Duration,
    pub num_code_by_hash: u64,
}

impl MeasuredProviderDb {
    pub fn new(provider: ProviderDb) -> Self {
        MeasuredProviderDb {
            provider,
            num_basic: 0,
            time_basic: Duration::default(),
            num_storage: 0,
            time_storage: Duration::default(),
            num_block_hash: 0,
            time_block_hash: Duration::default(),
            num_code_by_hash: 0,
        }
    }

    pub fn db(&mut self) -> &mut ProviderDb {
        &mut self.provider
    }

    pub fn print_report(&self) {
        println!("db accesses: ");
        print_duration(
            &format!("- account [{} ops]: ", self.num_basic),
            self.time_basic,
        );
        print_duration(
            &format!("- storage [{} ops]: ", self.num_storage),
            self.time_storage,
        );
        print_duration(
            &format!("- block_hash [{} ops]: ", self.num_block_hash),
            self.time_block_hash,
        );
        println!("- code_by_hash: {}", self.num_code_by_hash);
    }
}

impl Database for MeasuredProviderDb {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.num_basic += 1;
        let start = Instant::now();
        let res = self.provider.basic(address);
        self.time_basic.add_assign(start.elapsed());
        res
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.num_storage += 1;
        let start = Instant::now();
        let res = self.provider.storage(address, index);
        self.time_storage.add_assign(start.elapsed());
        res
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        self.num_block_hash += 1;
        let start = Instant::now();
        let res = self.provider.block_hash(number);
        self.time_block_hash.add_assign(start.elapsed());
        res
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.num_code_by_hash += 1;
        self.provider.code_by_hash(_code_hash)
    }
}

impl DatabaseCommit for MeasuredProviderDb {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.provider.commit(changes)
    }
}
