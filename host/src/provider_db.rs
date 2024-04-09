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
use alloy_primitives::{Bytes, StorageKey, Uint};
use alloy_provider::{Provider, ReqwestProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use raiko_lib::{
    builder::OptimisticDatabase, clear_line, consts::Network, inplace_print, mem_db::MemDb,
    taiko_utils::to_header,
};
use raiko_primitives::{Address, B256, U256};
use reqwest_alloy::Client;
use revm::{
    primitives::{Account, AccountInfo, Bytecode, HashMap},
    Database, DatabaseCommit,
};
use tokio::runtime::Handle;

use crate::preflight::get_block;

pub struct ProviderDb {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
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

type StorageProofs = HashMap<Address, EIP1186AccountProofResponse>;

impl ProviderDb {
    pub fn new(
        provider: ReqwestProvider,
        network: Network,
        block_number: u64,
    ) -> Result<Self, anyhow::Error> {
        let client = ClientBuilder::default()
            .reqwest_http(reqwest::Url::parse(&provider.client().transport().url()).unwrap());

        let mut provider_db = ProviderDb {
            provider,
            client,
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
        if network.is_taiko() {
            // Get the 256 history block hashes from the provider at first time for anchor
            // transaction.
            let start = block_number.saturating_sub(255);
            let block_numbers = (start..=block_number).collect();
            let initial_history_blocks = provider_db.fetch_blocks(&block_numbers)?;
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

    fn fetch_blocks(&mut self, block_numbers: &Vec<u64>) -> Result<Vec<Block>, anyhow::Error> {
        let mut all_blocks = Vec::new();

        let max_batch_size = 32;
        for block_numbers in block_numbers.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();
            let mut requests = vec![];

            for block_number in block_numbers.iter() {
                requests.push(Box::pin(batch.add_call(
                    "eth_getBlockByNumber",
                    &(BlockNumberOrTag::from(*block_number), false),
                )?));
            }

            let mut blocks = self.async_executor.block_on(async {
                batch.send().await?;
                let mut blocks = vec![];
                // Collect the data from the batch
                for request in requests.into_iter() {
                    blocks.push(request.await?);
                }
                Ok::<_, anyhow::Error>(blocks)
            })?;

            all_blocks.append(&mut blocks);
        }

        Ok(all_blocks)
    }

    fn fetch_accounts(&self, accounts: &Vec<Address>) -> Result<Vec<AccountInfo>, anyhow::Error> {
        let mut all_accounts = Vec::new();

        let max_batch_size = 250;
        for accounts in accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut nonce_requests = Vec::new();
            let mut balance_requests = Vec::new();
            let mut code_requests = Vec::new();

            for address in accounts {
                nonce_requests.push(Box::pin(
                    batch
                        .add_call::<_, Uint<64, 1>>(
                            "eth_getTransactionCount",
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
                balance_requests.push(Box::pin(
                    batch
                        .add_call::<_, Uint<256, 4>>(
                            "eth_getBalance",
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
                code_requests.push(Box::pin(
                    batch
                        .add_call::<_, Bytes>(
                            "eth_getCode",
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
            }

            let mut accounts = self.async_executor.block_on(async {
                batch.send().await?;
                let mut accounts = vec![];
                // Collect the data from the batch
                for (nonce_request, (balance_request, code_request)) in nonce_requests
                    .into_iter()
                    .zip(balance_requests.into_iter().zip(code_requests.into_iter()))
                {
                    let (nonce, balance, code) = (
                        nonce_request.await?,
                        balance_request.await?,
                        code_request.await?,
                    );

                    let account_info = AccountInfo::new(
                        balance,
                        nonce.try_into().unwrap(),
                        Bytecode::new_raw(code.clone()).hash_slow(),
                        Bytecode::new_raw(code),
                    );

                    accounts.push(account_info);
                }
                Ok::<_, anyhow::Error>(accounts)
            })?;

            all_accounts.append(&mut accounts);
        }

        Ok(all_accounts)
    }

    fn fetch_storage_slots(
        &self,
        accounts: &Vec<(Address, U256)>,
    ) -> Result<Vec<U256>, anyhow::Error> {
        let mut all_values = Vec::new();

        let max_batch_size = 1000;
        for accounts in accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut requests = Vec::new();

            for (address, key) in accounts {
                requests.push(Box::pin(
                    batch
                        .add_call::<_, U256>(
                            "eth_getStorageAt",
                            &(address, key, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
            }

            let mut values = self.async_executor.block_on(async {
                batch.send().await?;
                let mut values = vec![];
                // Collect the data from the batch
                for request in requests.into_iter() {
                    values.push(request.await?);
                }
                Ok::<_, anyhow::Error>(values)
            })?;

            all_values.append(&mut values);
        }

        Ok(all_values)
    }

    fn get_storage_proofs(
        &mut self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> Result<StorageProofs, anyhow::Error> {
        let mut storage_proofs = HashMap::new();
        let mut idx = offset;

        let mut accounts = accounts.clone();

        let batch_limit = 1000;
        while !accounts.is_empty() {
            inplace_print(&format!(
                "fetching storage proof {idx}/{num_storage_proofs}..."
            ));

            // Create a batch for all storage proofs
            let mut batch = self.client.new_batch();

            // Collect all requests
            let mut requests = Vec::new();

            let mut batch_size = 0;
            while !accounts.is_empty() && batch_size < batch_limit {
                let mut address_to_remove = None;
                if let Some((address, keys)) = accounts.iter_mut().next() {
                    // Calculate how many keys we can still process
                    let num_keys_to_process = if batch_size + keys.len() < batch_limit {
                        keys.len()
                    } else {
                        batch_limit - batch_size
                    };

                    // If we can process all keys, remove the address from the map after the loop
                    if num_keys_to_process == keys.len() {
                        address_to_remove = Some(address.clone());
                    }

                    // Extract the keys to process
                    let keys_to_process = keys
                        .drain(0..num_keys_to_process)
                        .map(|v| StorageKey::from(v))
                        .collect::<Vec<_>>();

                    // Add the request
                    requests.push(Box::pin(
                        batch
                            .add_call::<_, EIP1186AccountProofResponse>(
                                "eth_getProof",
                                &(
                                    address.clone(),
                                    keys_to_process.clone(),
                                    BlockId::from(block_number),
                                ),
                            )
                            .unwrap(),
                    ));

                    // Keep track of how many keys were processed
                    // Add an additional 1 for the account proof itself
                    batch_size += 1 + keys_to_process.len();
                }

                // Remove the address if all keys were processed for this account
                if let Some(address) = address_to_remove {
                    accounts.remove(&address);
                }
            }

            // Send the batch
            self.async_executor.block_on(async { batch.send().await })?;

            // Collect the data from the batch
            for request in requests.into_iter() {
                let mut proof = self.async_executor.block_on(async { request.await })?;
                idx += proof.storage_proof.len();
                if let Some(map_proof) = storage_proofs.get_mut(&proof.address) {
                    map_proof.storage_proof.append(&mut proof.storage_proof);
                } else {
                    storage_proofs.insert(proof.address, proof);
                }
            }
        }
        clear_line();

        Ok(storage_proofs)
    }

    pub fn get_proofs(&mut self) -> Result<(StorageProofs, StorageProofs), anyhow::Error> {
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

    pub fn is_valid_run(&self) -> bool {
        self.pending_accounts.is_empty()
            && self.pending_slots.is_empty()
            && self.pending_block_hashes.is_empty()
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
        let account = self.fetch_accounts(&vec![address])?[0].clone();

        // Insert the account into the initial database.
        self.initial_db
            .insert_account_info(address, account.clone());
        Ok(Some(account))
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
                    .insert_account_storage(&address, index, db_result.clone());
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
        let value = self.fetch_storage_slots(&vec![(address, index)])?[0].clone();

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
                self.initial_db
                    .insert_block_hash(block_number, db_result.clone());
            }
            return Ok(db_result);
        }

        // In optimistic mode, don't wait on the data and just return some default values
        if self.optimistic {
            self.pending_block_hashes.insert(block_number);
            return Ok(B256::default());
        }

        // Fetch the block hash
        let block_hash = self.fetch_blocks(&vec![block_number])?[0]
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

impl DatabaseCommit for ProviderDb {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.current_db.commit(changes)
    }
}

impl OptimisticDatabase for ProviderDb {
    fn fetch_data(&mut self) -> bool {
        //println!("all accounts touched: {:?}", self.pending_accounts);
        //println!("all slots touched: {:?}", self.pending_slots);
        //println!("all block hashes touched: {:?}", self.pending_block_hashes);

        // This run was valid when no pending work was scheduled
        let valid_run = self.is_valid_run();

        let accounts = self
            .fetch_accounts(&self.pending_accounts.iter().cloned().collect())
            .unwrap();
        for (address, account) in take(&mut self.pending_accounts)
            .into_iter()
            .zip(accounts.iter())
        {
            self.staging_db
                .insert_account_info(address.clone(), account.clone());
        }

        let slots = self
            .fetch_storage_slots(&self.pending_slots.iter().cloned().collect())
            .unwrap();
        for ((address, index), value) in take(&mut self.pending_slots).into_iter().zip(slots.iter())
        {
            self.staging_db
                .insert_account_storage(&address, index.clone(), value.clone());
        }

        let blocks = self
            .fetch_blocks(&self.pending_block_hashes.iter().cloned().collect())
            .unwrap();
        for (block_number, block) in take(&mut self.pending_block_hashes)
            .into_iter()
            .zip(blocks.iter())
        {
            self.staging_db
                .insert_block_hash(block_number, block.header.hash.unwrap().0.into());
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
