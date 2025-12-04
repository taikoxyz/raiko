use alloy_consensus::TrieAccount;
use alloy_primitives::map::AddressMap;
use reth_stateless::StatelessTrie;

pub trait StatelessTrieExt: StatelessTrie {
    fn append_callers(&mut self, callers: AddressMap<TrieAccount>);
}
