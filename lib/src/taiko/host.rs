use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    taiko::*,
    transactions::{ethereum::EthereumTxEssence, TxEssence},
    Address, B256,
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
    pub extra: TaikoExtra,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaikoExtra {
    pub l2_tx_list: Vec<u8>,
    pub l1_hash: B256,
    pub l1_height: u64,
    pub l2_parent_gas_used: u32,
    pub prover: Address,
    pub graffiti: B256,
    pub l1_signal_root: B256,
    pub l2_signal_root: B256,
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
    let anchorCall {
        l1Hash: l1_hash,
        l1SignalRoot: l1_signal_root,
        l1Height: l1_height,
        parentGasUsed: l2_parent_gas_used,
    } = decode_anchor_call_args(&l2_init.fini_transactions[0].essence.data())
        .context("failed to decode anchor arguments")?;
    let l1_init = get_initial_data::<N>(
        l1_chain_spec,
        l1_cache_path,
        l1_rpc_url,
        l1_height,
        Some(l2_block_no),
        *L1_SIGNAL_SERVICE,
    )?;
    let propose_block_call = decode_propose_block_call_args(l1_init.propose.as_ref().unwrap())
        .context("failed to get tx list from propose block tx")?;
    let l2_signal_root = l2_init.signal_root;
    let mut init = TaikoInit {
        l1_init,
        l2_init,
        extra: TaikoExtra {
            l2_tx_list: propose_block_call.txList,
            l1_hash,
            l1_height,
            prover,
            graffiti,
            l1_signal_root,
            l2_signal_root,
            l2_parent_gas_used,
        },
    };
    // rebuild transaction list by tx_list from l1 contract
    precheck_block(&mut init)?;
    Ok(init)
}
