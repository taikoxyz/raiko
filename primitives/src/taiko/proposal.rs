use alloy_sol_types::{sol, SolCall};
use anyhow::{Context, Result};

use crate::transactions::{Transaction, TxEssence};

sol! {
    function proposeBlock(
        bytes calldata input,
        bytes calldata assignment,
        bytes calldata txList
    )
    {}
}

pub fn decode_propose_block_call_args<E: TxEssence>(
    tx: &Transaction<E>,
) -> Result<proposeBlockCall> {
    let data = tx.essence.data();
    let propose_block_call =
        proposeBlockCall::abi_decode(data, false).context("failed to decode propose block call")?;
    Ok(propose_block_call)
}
