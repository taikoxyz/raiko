use alloy_primitives::{Address, U256};
use alloy_rpc_types::Block;
use revm::primitives::AccountInfo;
use std::collections::HashMap;

use crate::{interfaces::RaikoResult, MerkleProof};

pub mod db;
pub mod rpc;

#[allow(async_fn_in_trait)]
pub trait BlockDataProvider {
    async fn get_block(&self, block_number: u64) -> RaikoResult<Block>;

    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>>;

    async fn get_accounts(&self, accounts: &[Address]) -> RaikoResult<Vec<AccountInfo>>;

    async fn get_storage_values(&self, accounts: &[(Address, U256)]) -> RaikoResult<Vec<U256>>;

    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> RaikoResult<MerkleProof>;
}
