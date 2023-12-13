use anyhow::Result;
use zeth_primitives::{
    block::Header,
    keccak,
    taiko::{
        deposits_hash, string_to_bytes32, BlockMetadata, EthDeposit, ProtocolInstance, Transition,
        TIER_SGX_ID,
    },
    TxHash,
};

use crate::taiko::host::TaikoExtra;

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
    let pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            signalRoot: extra.l2_signal_root,
            graffiti: extra.graffiti,
        },
        block_metadata: BlockMetadata {
            l1Hash: extra.l1_hash,
            difficulty: header.difficulty.into(),
            blobHash: tx_list_hash,
            extraData: extra_data.into(),
            depositsHash: deposits_hash,
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: header.gas_limit.try_into().unwrap(),
            timestamp: header.timestamp.try_into().unwrap(),
            l1Height: extra.l1_height,
            txListByteOffset: 0u32,
            txListByteSize: 0u32,
            minTier: TIER_SGX_ID,
            blobUsed: extra.l2_tx_list.is_empty(),
            parentMetaHash: extra.block_proposed.meta.parentMetaHash,
        },
        prover: extra.prover,
    };
    #[cfg(not(target_os = "zkvm"))]
    {
        use zeth_primitives::taiko::assert_pi_and_bp;
        assert_pi_and_bp(&pi, &extra.block_proposed)?;
    }
    Ok(pi)
}
