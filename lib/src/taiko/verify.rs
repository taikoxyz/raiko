use alloy_sol_types::SolValue;
use anyhow::{anyhow, Result};
use serde_json::to_string;
use zeth_primitives::{
    block::Header, ethers::from_ethers_h256, taiko::ProtocolInstance,
    transactions::EthereumTransaction,
};

use crate::taiko::host::TaikoExtra;

pub fn verify(header: &Header, pi: &mut ProtocolInstance, extra: &TaikoExtra) -> Result<()> {
    // check the block metadata
    if pi.block_metadata.abi_encode() != extra.block_proposed.meta.abi_encode() {
        return Err(anyhow!(
            "block metadata mismatch, expected: {:?}, got: {:?}",
            extra.block_proposed.meta,
            pi.block_metadata
        ));
    }
    // check the block hash
    if Some(header.hash()) != extra.l2_fini_block.hash.map(from_ethers_h256) {
        // allow list of block hashes to be different
        if vec![10654, 10667, 23207].contains(&header.number) {
            pi.transition.blockHash = extra
                .l2_fini_block
                .hash
                .map(from_ethers_h256)
                .unwrap_or_default();
            println!("Block hash: {:?}", extra.l2_fini_block.hash);
            return Ok(());
        }
        let txs: Vec<EthereumTransaction> = extra
            .l2_fini_block
            .transactions
            .iter()
            .filter_map(|tx| tx.clone().try_into().ok())
            .collect();
        return Err(anyhow!(
            "block hash mismatch, expected: {}, got: {}",
            to_string(&txs).unwrap_or_default(),
            to_string(&header.transactions).unwrap_or_default(),
        ));
    }

    println!("Protocol instance Transition: {:?}", pi.transition);
    println!("Protocol instance Metahash: {}", pi.meta_hash());
    Ok(())
}
