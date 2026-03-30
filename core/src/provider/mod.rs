use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types::{Block, TransactionRequest};
use raiko_lib::consts::SupportedChainSpecs;
use reth_revm::state::AccountInfo;
use std::collections::HashMap;

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::rpc::{PrestateTraceResult, RpcBlockDataProvider},
    MerkleProof,
};

pub mod db;
pub mod rpc;

#[allow(async_fn_in_trait)]
pub trait BlockDataProvider: Clone + std::fmt::Debug {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>>;

    async fn get_accounts(
        &self,
        block_number: u64,
        accounts: &[Address],
    ) -> RaikoResult<Vec<AccountInfo>>;

    async fn get_storage_values(
        &self,
        block_number: u64,
        accounts: &[(Address, U256)],
    ) -> RaikoResult<Vec<U256>>;

    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> RaikoResult<MerkleProof>;

    /// Pre-fetch all accounts and storage slots accessed by the given transactions using
    /// eth_createAccessList. Returns (addresses, slots) to pre-populate the staging DB.
    async fn get_access_list_for_txs(
        &self,
        block_number: u64,
        tx_requests: &[TransactionRequest],
    ) -> RaikoResult<(Vec<Address>, Vec<(Address, U256)>)>;

    /// Trace an entire block with prestateTracer to get ALL state accessed during execution.
    /// Returns None if the provider does not support debug APIs.
    async fn trace_block_prestate(
        &self,
        _block_number: u64,
    ) -> Option<RaikoResult<PrestateTraceResult>> {
        None
    }
}

pub async fn get_task_data(
    network: &str,
    block_number: u64,
    chain_specs: &SupportedChainSpecs,
) -> RaikoResult<(u64, B256)> {
    let taiko_chain_spec = chain_specs
        .get_chain_spec(network)
        .ok_or_else(|| RaikoError::InvalidRequestConfig("Unsupported raiko network".to_string()))?;
    let provider = RpcBlockDataProvider::new(&taiko_chain_spec.rpc.clone()).await?;
    let blocks = provider.get_blocks(&[(block_number, true)]).await?;
    let block = blocks
        .first()
        .ok_or_else(|| RaikoError::RPC("No block for requested block number".to_string()))?;
    let blockhash = block.header.hash;
    Ok((taiko_chain_spec.chain_id, blockhash))
}
