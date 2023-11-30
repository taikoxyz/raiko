use alloy_primitives::B256;
use alloy_sol_types::{sol, Error, SolCall, SolType};
use anyhow::{Context, Result};
use ethers_core::types::{Address, U256};

use super::{consts::*, protocol_instance::BlockMetadata};
use crate::transactions::{Transaction, TxEssence};

sol! {
    function proposeBlock(
        bytes calldata input,
        bytes calldata assignment,
        bytes calldata txList
    )
    {}
}

pub fn decode_propose_block_call_args<E: TxEssence>(tx: &Transaction<E>) -> Result<Vec<u8>> {
    let data = tx.essence.data();
    let propose_block =
        proposeBlockCall::abi_decode(data, false).context("failed to decode propose block call")?;
    let tx_list = propose_block.txList;
    Ok(tx_list)
}
