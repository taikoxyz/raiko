use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types::Block;
use raiko_lib::consts::SupportedChainSpecs;
use reth_primitives::revm_primitives::AccountInfo;
use std::collections::HashMap;

use crate::{
    interfaces::{RaikoError, RaikoResult},
    preflight::parse_l1_batch_proposal_tx_for_pacaya_fork,
    provider::rpc::RpcBlockDataProvider,
    MerkleProof,
};

pub mod db;
pub mod rpc;

#[allow(async_fn_in_trait)]
pub trait BlockDataProvider {

    async fn set_chain(&self, chain_id: u64) -> RaikoResult<bool>;

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
}

pub async fn get_task_data(
    network: &str,
    block_number: u64,
    chain_specs: &SupportedChainSpecs,
) -> RaikoResult<(u64, B256)> {
    println!("task data network: {:?}", network);
    println!("chain_specs: {:?}", chain_specs);
    let taiko_chain_spec = chain_specs
        .get_chain_spec(network)
        .ok_or_else(|| RaikoError::InvalidRequestConfig("Unsupported raiko network".to_string()))?;
    let provider =
        RpcBlockDataProvider::new(&taiko_chain_spec.rpc.clone(), block_number - 1).await?;
    let blocks = provider.get_blocks(&[(block_number, true)]).await?;
    let block = blocks
        .first()
        .ok_or_else(|| RaikoError::RPC("No block for requested block number".to_string()))?;
    let blockhash = block
        .header
        .hash;
    Ok((taiko_chain_spec.chain_id, blockhash))
}

pub async fn get_batch_task_data(
    network: &str,
    l1_network: &str,
    batch_id: u64,
    l1_inclusion_block_number: u64,
    chain_specs: &SupportedChainSpecs,
) -> RaikoResult<(u64, B256)> {
    let l1_chain_spec = chain_specs
        .get_chain_spec(l1_network)
        .ok_or_else(|| RaikoError::InvalidRequestConfig("Unsupported l1 network".to_string()))?;
    let taiko_chain_spec = chain_specs
        .get_chain_spec(network)
        .ok_or_else(|| RaikoError::InvalidRequestConfig("Unsupported raiko network".to_string()))?;
    let all_prove_blocks = parse_l1_batch_proposal_tx_for_pacaya_fork(
        &l1_chain_spec,
        &taiko_chain_spec,
        l1_inclusion_block_number,
        batch_id,
    )
    .await?;

    let batch_block_number_start = all_prove_blocks.first().expect("No block numbers provided");
    let provider =
        RpcBlockDataProvider::new(&taiko_chain_spec.rpc.clone(), batch_block_number_start - 1)
            .await?;
    let blocks = provider
        .get_blocks(&[(*batch_block_number_start, false)])
        .await?;
    let block = blocks
        .first()
        .ok_or_else(|| RaikoError::RPC("No block for requested block number".to_string()))?;
    let blockhash = block
        .header
        .hash;
    Ok((taiko_chain_spec.chain_id, blockhash))
}
