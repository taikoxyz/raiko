#![allow(async_fn_in_trait)]

pub mod network;

use alloy_primitives::{map::AddressMap, Address};
use alloy_trie::TrieAccount;
use raiko2_primitives::RaizenResult;
use reth_ethereum_primitives::Block;
use reth_stateless::ExecutionWitness;

pub use network::NetworkProvider;

/// The `Provider` trait defines asynchronous methods for batch retrieval of blockchain data.
///
/// Implementors of this trait are responsible for providing access to blocks, accounts, and execution witnesses
/// for given block numbers and account addresses.
///
/// # Methods
///
/// - [`batch_blocks`]: Fetches a batch of blocks corresponding to the provided block numbers.
/// - [`batch_accounts`]: Fetches account state data for multiple blocks and sets of addresses.
/// - [`batch_witnesses`]: Fetches execution witnesses for a batch of blocks.
///
/// All methods return a [`RaizenResult`] wrapping the respective data type.
pub trait Provider {
    async fn batch_blocks(&self, blocks: &[u64]) -> RaizenResult<Vec<Block>>;

    async fn batch_accounts(
        &self,
        blocks: &[u64],
        accounts: &[Vec<Address>],
    ) -> RaizenResult<Vec<AddressMap<TrieAccount>>>;

    async fn batch_witnesses(&self, blocks: &[u64]) -> RaizenResult<Vec<ExecutionWitness>>;
}
