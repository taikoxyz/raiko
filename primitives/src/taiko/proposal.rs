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

pub fn get_tx_list<E: TxEssence>(tx: &Transaction<E>) -> Result<Vec<u8>> {
    let data = tx.essence.data();
    let propose_block =
        proposeBlockCall::abi_decode(data, false).context("failed to decode propose block call")?;
    let tx_list = propose_block.txList;
    Ok(tx_list)
}
// pub fn filter_proposal_tx<E: TxEssence>(txs: &[Transaction<E>]) ->
// Result<Transaction<E>> {     for tx in txs {
//         // 1. check to address
//         if tx.essence.to() != *L1_CONTRACT {
//             continue;
//         }
//         // 2. check data
//         let data = tx.essence.data();
//         let proposal =
//            match proposeBlockCall::abi_decode(data, false) {
//             Ok(_) => todo!(),
//             Err(Error::SI => {
//                 if err
//             },
//         };
//         let block_metadata =
//             BlockMetadata::abi_decode(data, false).context("invalid block metadata")?;
//     }
// }
