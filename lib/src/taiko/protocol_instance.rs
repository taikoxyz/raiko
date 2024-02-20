use anyhow::Result;
use ethers_core::{
    abi::{encode_packed, Token},
    types::U256 as EthU256,
};
use zeth_primitives::{
    block::Header,
    ethers::from_ethers_h256,
    keccak,
    taiko::{
        deposits_hash, string_to_bytes32, BlockMetadata, EthDeposit, ProtocolInstance, Transition,
        ANCHOR_GAS_LIMIT,
    },
    TxHash,
};
use alloy_sol_types::SolValue;

use crate::taiko::host::TaikoExtra;


fn calc_difficulty(prevrando: EthU256, num_blocks: u64, block_num: EthU256) -> [u8; 32] {
    // meta.difficulty = keccak256(abi.encodePacked(block.prevrandao, b.numBlocks, block.number));
    let prevrando_bytes: [u8; 32] = prevrando.into();
    let num_blocks_bytes: [u8; 8] = num_blocks.to_be_bytes().to_vec().try_into().unwrap();
    let block_num_bytes: [u8; 32] = block_num.into();
    let packed_bytes = (
        prevrando_bytes,
        num_blocks_bytes,
        block_num_bytes,
    ).abi_encode_packed();
    keccak::keccak(packed_bytes)
}

pub fn assemble_protocol_instance(extra: &TaikoExtra, header: &Header) -> Result<ProtocolInstance> {
    let tx_list_hash = TxHash::from(keccak::keccak(extra.l2_tx_list.as_slice()));
    let deposits: Vec<EthDeposit> = extra
        .l2_withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect();
    let deposits_hash = deposits_hash(&deposits);
    let extra_data = string_to_bytes32(&header.extra_data);
    let prevrando: EthU256 = if cfg!(feature = "pos") {
        extra.l1_next_block.mix_hash.unwrap_or_default().0.into()
    } else {
        extra.l1_next_block.difficulty
    };
    let difficulty = calc_difficulty(
        prevrando,
        header.number,
        EthU256::from(extra.l1_next_block.number.unwrap_or_default().as_u64()),
    );
    let gas_limit: u64 = header.gas_limit.try_into().unwrap();
    let mut pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            stateRoot: from_ethers_h256(extra.l2_fini_block.state_root),
            graffiti: extra.graffiti,
            __reserved: Default::default(),
        },
        block_metadata: BlockMetadata {
            l1Hash: extra.l1_hash,
            difficulty: difficulty.into(),
            blobHash: tx_list_hash,
            extraData: extra_data.into(),
            depositsHash: deposits_hash,
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (gas_limit - ANCHOR_GAS_LIMIT) as u32,
            timestamp: header.timestamp.try_into().unwrap(),
            l1Height: extra.l1_height,
            txListByteOffset: 0u32,
            txListByteSize: extra.l2_tx_list.len() as u32,
            minTier: extra.block_proposed.meta.minTier,
            blobUsed: extra.l2_tx_list.is_empty(),
            parentMetaHash: extra.block_proposed.meta.parentMetaHash,
        },
        prover: extra.prover,
    };
    #[cfg(not(target_os = "zkvm"))]
    {
        crate::taiko::verify::verify(header, &mut pi, extra)?;
    }
    Ok(pi)
}

#[cfg(test)]
mod test {
    use ethers_core::types::U256;

    use super::*;

    #[test]
    fn test_assemble_protocol_difficulty() {
        let difficulty = calc_difficulty(U256::from(1234u64), 5678u64, U256::from(4321u64));
        assert_eq!(
            hex::encode(difficulty),
            "ed29e631be1dc988025d0e874bf84fe27894c9c0a8034b3a0a212ccbf4216a79"
        );

        let difficulty = calc_difficulty(U256::from(0), 0, U256::from(0));
        assert_eq!(
            hex::encode(difficulty),
            "3cac317908c699fe873a7f6ee4e8cd63fbe9918b2315c97be91585590168e301"
        );
    }
}
