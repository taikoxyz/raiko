use std::fmt::Debug;

use anyhow::{Context, Result};
use ethers_core::types::{Transaction as EthersTransaction, H256};
use zeth_primitives::{
    taiko::*,
    transactions::{Transaction, TxEssence},
    Address, TxHash, B256,
};

use crate::{
    block_builder::NetworkStrategyBundle,
    consts::ChainSpec,
    host::{get_initial_data, provider::new_provider, Init},
};

#[derive(Clone)]
pub struct TaikoInit<E: TxEssence> {
    pub l1_init: Init<E>,
    pub l2_init: Init<E>,
    pub tx_list: Vec<u8>,
    pub l1_hash: B256,
    pub l1_height: u64,
    pub prover: Address,
}

pub fn get_taiko_initial_data<N: NetworkStrategyBundle>(
    l1_cache_path: Option<String>,
    l1_chain_spec: ChainSpec,
    l1_rpc_url: Option<String>,
    propose_block_tx: H256,
    prover: Address,
    l2_cache_path: Option<String>,
    l2_chain_spec: ChainSpec,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
) -> Result<TaikoInit<N::TxEssence>>
where
    N::TxEssence: TryFrom<EthersTransaction>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    let l2_init =
        get_initial_data::<N>(l2_chain_spec, l2_cache_path, l2_rpc_url, l2_block_no, None)?;
    // TODO: check l1 signal root
    let (l1_hash, _l1_signal_root, l1_height, _parent_gas_used) =
        decode_anchor_arguments(&l2_init.fini_transactions[0].essence.data())
            .context("failed to decode anchor arguments")?;
    let l1_init = get_initial_data::<N>(
        l1_chain_spec,
        l1_cache_path,
        l1_rpc_url,
        l1_height,
        Some(propose_block_tx),
    )?;
    let tx_list = get_tx_list(l1_init.transaction.as_ref().unwrap())
        .context("failed to get tx list from propose block tx")?;
    Ok(TaikoInit {
        l1_init,
        l2_init,
        tx_list,
        l1_hash,
        l1_height,
        prover,
    })
}
