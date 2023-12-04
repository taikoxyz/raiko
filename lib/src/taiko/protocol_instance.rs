use anyhow::Result;
use zeth_primitives::{
    block::Header,
    keccak,
    taiko::{string_to_bytes32, BlockEvidence, BlockMetadata, EthDeposit, ProtocolInstance},
    TxHash,
};

use crate::taiko::host::TaikoExtra;

pub fn assemble_protocol_instance(extra: &TaikoExtra, header: &Header) -> Result<ProtocolInstance> {
    let tx_list_hash = TxHash::from(keccak::keccak(extra.l2_tx_list.as_slice()));
    let deposits = extra
        .l2_withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect();
    let extra_data = string_to_bytes32(&header.extra_data);
    let pi = ProtocolInstance {
        block_evidence: BlockEvidence {
            blockMetadata: BlockMetadata {
                l1Hash: extra.l1_hash,
                difficulty: header.difficulty.into(),
                txListHash: tx_list_hash,
                extraData: extra_data.into(),
                id: header.number,
                timestamp: header.timestamp.try_into().unwrap(),
                l1Height: extra.l1_height,
                gasLimit: header.gas_limit.try_into().unwrap(),
                coinbase: header.beneficiary,
                depositsProcessed: deposits,
            },
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            signalRoot: extra.l2_signal_root,
            graffiti: extra.graffiti,
        },
        prover: extra.prover,
    };
    Ok(pi)
}
