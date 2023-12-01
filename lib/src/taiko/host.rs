use anyhow::{Context, Result};
use zeth_primitives::{
    taiko::*,
    transactions::{ethereum::EthereumTxEssence, TxEssence},
    Address, B256, U256,
};

use crate::{
    block_builder::NetworkStrategyBundle,
    consts::ChainSpec,
    host::{get_initial_data, Init},
    taiko::precheck::precheck_block,
};

#[derive(Clone)]
pub struct TaikoInit<E: TxEssence> {
    pub l1_init: Init<E>,
    pub l2_init: Init<E>,
    pub tx_list: Vec<u8>,
    pub l1_hash: B256,
    pub l1_height: u64,
    pub prover: Address,
    pub graffiti: B256,
    pub signal_root: B256,
}

pub fn get_taiko_initial_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    l1_cache_path: Option<String>,
    l1_chain_spec: ChainSpec,
    l1_rpc_url: Option<String>,
    prover: Address,
    l2_cache_path: Option<String>,
    l2_chain_spec: ChainSpec,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
    graffiti: B256,
) -> Result<TaikoInit<N::TxEssence>> {
    let l2_init = get_initial_data::<N>(
        l2_chain_spec,
        l2_cache_path,
        l2_rpc_url,
        l2_block_no,
        None,
        *L2_SIGNAL_SERVICE,
    )?;
    let (l1_hash, l1_signal_root, l1_height, parent_gas_used) =
        decode_anchor_call_args(&l2_init.fini_transactions[0].essence.data())
            .context("failed to decode anchor arguments")?;
    if l2_init.init_block.gas_used != U256::from(parent_gas_used) {
        return Err(anyhow::anyhow!("parent gas used mismatch"));
    }
    let l1_init = get_initial_data::<N>(
        l1_chain_spec,
        l1_cache_path,
        l1_rpc_url,
        l1_height,
        Some(l2_block_no),
        *L1_SIGNAL_SERVICE,
    )?;
    if l1_signal_root != l1_init.signal_root {
        return Err(anyhow::anyhow!("l1 signal root mismatch"));
    }
    if l1_init.fini_block.hash() != l1_hash {
        return Err(anyhow::anyhow!("l1 block hash mismatch"));
    }
    let tx_list = decode_propose_block_call_args(l1_init.propose.as_ref().unwrap())
        .context("failed to get tx list from propose block tx")?;
    let signal_root = l2_init.signal_root;
    let mut init = TaikoInit {
        l1_init,
        l2_init,
        tx_list,
        l1_hash,
        l1_height,
        prover,
        graffiti,
        signal_root,
    };
    // rebuild transaction list by tx_list from l1 contract
    precheck_block(&mut init)?;
    Ok(init)
}
