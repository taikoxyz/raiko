use alloy_primitives::B256;
use alloy_rpc_types::Block;
use raiko_lib::consts::SupportedChainSpecs;

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::rpc::{ExecutionWitness, RpcBlockDataProvider},
};

pub mod rpc;

#[allow(async_fn_in_trait)]
pub trait BlockDataProvider: Clone + std::fmt::Debug {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>>;

    /// Fetch the execution witness for a block via debug_executionWitness.
    /// Returns None if the provider does not support this API.
    async fn execution_witness(&self, _block_number: u64) -> Option<RaikoResult<ExecutionWitness>> {
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
