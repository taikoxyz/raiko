use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types::Block;
use raiko_lib::consts::SupportedChainSpecs;
use reth_primitives::revm_primitives::AccountInfo;
use std::collections::HashMap;

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::{ipc::IpcBlockDataProvider, rpc::RpcBlockDataProvider},
    MerkleProof,
};

pub mod db;
pub mod ipc;
pub mod rpc;

#[allow(async_fn_in_trait)]
pub trait BlockDataProvider {
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

pub async fn get_task_data(
    network: &str,
    block_number: u64,
    chain_specs: &SupportedChainSpecs,
) -> RaikoResult<(u64, B256)> {
    let taiko_chain_spec = chain_specs
        .get_chain_spec(network)
        .ok_or_else(|| RaikoError::InvalidRequestConfig("Unsupported raiko network".to_string()))?;
    let provider = RpcBlockDataProvider::new(&taiko_chain_spec.rpc.clone(), block_number - 1)?;
    let blocks = provider.get_blocks(&[(block_number, true)]).await?;
    let block = blocks
        .first()
        .ok_or_else(|| RaikoError::RPC("No block for requested block number".to_string()))?;
    let blockhash = block
        .header
        .hash
        .ok_or_else(|| RaikoError::RPC("No block hash for requested block".to_string()))?;
    Ok((taiko_chain_spec.chain_id, blockhash))
}

pub enum BlockDataProviderType {
    Rpc(RpcBlockDataProvider),
    Ipc(IpcBlockDataProvider),
}

impl BlockDataProviderType {
    pub async fn new(url: &str, block_number: u64, use_ipc: bool) -> RaikoResult<Self> {
        if use_ipc {
            Ok(Self::Ipc(
                IpcBlockDataProvider::new(url, block_number).await?,
            ))
        } else {
            Ok(Self::Rpc(RpcBlockDataProvider::new(url, block_number)?))
        }
    }
}

impl BlockDataProvider for BlockDataProviderType {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>> {
        match self {
            Self::Rpc(provider) => provider.get_blocks(blocks_to_fetch).await,
            Self::Ipc(provider) => provider.get_blocks(blocks_to_fetch).await,
        }
    }

    async fn get_accounts(&self, accounts: &[Address]) -> RaikoResult<Vec<AccountInfo>> {
        match self {
            Self::Rpc(provider) => provider.get_accounts(accounts).await,
            Self::Ipc(provider) => provider.get_accounts(accounts).await,
        }
    }

    async fn get_storage_values(&self, accounts: &[(Address, U256)]) -> RaikoResult<Vec<U256>> {
        match self {
            Self::Rpc(provider) => provider.get_storage_values(accounts).await,
            Self::Ipc(provider) => provider.get_storage_values(accounts).await,
        }
    }

    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> RaikoResult<MerkleProof> {
        match self {
            Self::Rpc(provider) => {
                provider
                    .get_merkle_proofs(block_number, accounts, offset, num_storage_proofs)
                    .await
            }
            Self::Ipc(provider) => {
                provider
                    .get_merkle_proofs(block_number, accounts, offset, num_storage_proofs)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod test {
    use alloy_primitives::Address;
    use raiko_lib::{input::BlobProofType, proof_type::ProofType};
    use reth_primitives::B256;

    use crate::{
        interfaces::ProofRequest,
        preflight::sidecar::{GuestInputProvider, GuestInputProviderImpl},
    };

    #[ignore = "too many output"]
    #[tokio::test]
    async fn test_gip() {
        let gip = GuestInputProviderImpl {};
        let url = "http://34.124.151.77:8888";
        let proof_type = ProofType::Native;
        let l1_network = "ethereum".to_owned();
        let network = "taiko_mainnet".to_owned();
        let block_number = std::env::var("BLOCK_NUMBER")
            .unwrap_or_else(|_| "1".to_string())
            .parse::<u64>()
            .unwrap();

        let proof_request = ProofRequest {
            block_number,
            l1_inclusion_block_number: 0,
            network,
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            l1_network,
            proof_type,
            blob_proof_type: BlobProofType::ProofOfEquivalence,
            prover_args: Default::default(),
        };
        let response = gip.get_guest_input(url, &proof_request).await;
        println!("response: {:?}", response);
    }
}
