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

use anyhow::Result;
use ethers_core::types::{Block, Bytes, EIP1186ProofResponse, Transaction, H256, U256};
#[cfg(feature = "taiko")]
use zeth_primitives::taiko::BlockProposed;

use crate::host::provider::{
    rpc_provider::RpcProvider, AccountQuery, BlockQuery, MutProvider, ProofQuery, ProposeQuery,
    Provider, StorageQuery,
};

pub struct CachedRpcProvider {
    cache: Box<dyn MutProvider>,
    rpc: RpcProvider,
}

impl CachedRpcProvider {
    pub fn new(cache: Box<dyn MutProvider>, rpc_url: String) -> Result<Self> {
        let rpc = RpcProvider::new(rpc_url)?;

        Ok(CachedRpcProvider { cache, rpc })
    }
}

impl Provider for CachedRpcProvider {
    fn save(&self) -> Result<()> {
        Ok(())
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block<Transaction>> {
        let cache_out = self.cache.get_full_block(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_full_block(query)?;
        self.cache.insert_full_block(query.clone(), out.clone());

        Ok(out)
    }

    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>> {
        let cache_out = self.cache.get_partial_block(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_partial_block(query)?;
        self.cache.insert_partial_block(query.clone(), out.clone());

        Ok(out)
    }

    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186ProofResponse> {
        let cache_out = self.cache.get_proof(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_proof(query)?;
        self.cache.insert_proof(query.clone(), out.clone());

        Ok(out)
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256> {
        let cache_out = self.cache.get_transaction_count(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_transaction_count(query)?;
        self.cache.insert_transaction_count(query.clone(), out);

        Ok(out)
    }

    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256> {
        let cache_out = self.cache.get_balance(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_balance(query)?;
        self.cache.insert_balance(query.clone(), out);

        Ok(out)
    }

    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes> {
        let cache_out = self.cache.get_code(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_code(query)?;
        self.cache.insert_code(query.clone(), out.clone());

        Ok(out)
    }

    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256> {
        let cache_out = self.cache.get_storage(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_storage(query)?;
        self.cache.insert_storage(query.clone(), out);

        Ok(out)
    }

    fn get_propose(&mut self, query: &ProposeQuery) -> Result<(Transaction, BlockProposed)> {
        let cache_out = self.cache.get_propose(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_propose(query)?;
        self.cache.insert_propose(query.clone(), out.clone());

        Ok(out)
    }

    fn batch_get_partial_blocks(&mut self, query: &BlockQuery) -> Result<Vec<Block<H256>>> {
        let cache_out = self.cache.batch_get_partial_blocks(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.batch_get_partial_blocks(query)?;
        for block in out.iter() {
            self.cache.insert_partial_block(
                BlockQuery {
                    block_no: block.number.unwrap().as_u64(),
                },
                block.clone(),
            );
        }
        Ok(out)
    }
}
