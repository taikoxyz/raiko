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

use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Result};
use ethers_core::types::{Block, Bytes, EIP1186ProofResponse, Transaction, H256, U256};
use zeth_primitives::taiko::BlockProposed;

use crate::host::provider::{
    AccountQuery, BlockQuery, MutProvider, ProofQuery, ProposeQuery, Provider, StorageQuery,
};

#[derive(Debug)]
pub struct MemProvider {
    full_blocks: HashMap<BlockQuery, Block<Transaction>>,
    partial_blocks: BTreeMap<BlockQuery, Block<H256>>,
    proofs: HashMap<ProofQuery, EIP1186ProofResponse>,
    transaction_count: HashMap<AccountQuery, U256>,
    balance: HashMap<AccountQuery, U256>,
    code: HashMap<AccountQuery, Bytes>,
    storage: HashMap<StorageQuery, H256>,
    propose: Option<(Transaction, BlockProposed)>,
}

impl MemProvider {
    pub fn new() -> Self {
        MemProvider {
            full_blocks: HashMap::new(),
            partial_blocks: BTreeMap::new(),
            proofs: HashMap::new(),
            transaction_count: HashMap::new(),
            balance: HashMap::new(),
            code: HashMap::new(),
            storage: HashMap::new(),
            propose: Default::default(),
        }
    }
}

impl Provider for MemProvider {
    fn save(&self) -> Result<()> {
        Ok(())
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block<Transaction>> {
        match self.full_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>> {
        match self.partial_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186ProofResponse> {
        match self.proofs.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256> {
        match self.transaction_count.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256> {
        match self.balance.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes> {
        match self.code.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256> {
        match self.storage.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_propose(&mut self, query: &ProposeQuery) -> Result<(Transaction, BlockProposed)> {
        match self.propose {
            Some(ref val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn batch_get_partial_blocks(&mut self, _: &BlockQuery) -> Result<Vec<Block<H256>>> {
        if self.partial_blocks.is_empty() {
            Err(anyhow!("No data for partial blocks"))
        } else {
            Ok(self.partial_blocks.values().cloned().collect())
        }
    }
}

impl MutProvider for MemProvider {
    fn insert_full_block(&mut self, query: BlockQuery, val: Block<Transaction>) {
        self.full_blocks.insert(query, val);
    }

    fn insert_partial_block(&mut self, query: BlockQuery, val: Block<H256>) {
        self.partial_blocks.insert(query, val);
    }

    fn insert_proof(&mut self, query: ProofQuery, val: EIP1186ProofResponse) {
        self.proofs.insert(query, val);
    }

    fn insert_transaction_count(&mut self, query: AccountQuery, val: U256) {
        self.transaction_count.insert(query, val);
    }

    fn insert_balance(&mut self, query: AccountQuery, val: U256) {
        self.balance.insert(query, val);
    }

    fn insert_code(&mut self, query: AccountQuery, val: Bytes) {
        self.code.insert(query, val);
    }

    fn insert_storage(&mut self, query: StorageQuery, val: H256) {
        self.storage.insert(query, val);
    }

    fn insert_propose(&mut self, _query: ProposeQuery, val: (Transaction, BlockProposed)) {
        self.propose = Some(val);
    }
}
