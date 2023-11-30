use anyhow::Result;
use zeth_primitives::{
    block::Header,
    keccak,
    taiko::{string_to_bytes32, BlockEvidence, BlockMetadata, EthDeposit, ProtocolInstance},
    transactions::TxEssence,
    TxHash,
};

use crate::taiko::input::TaikoInput;

pub fn assemble_protocol_instance<E: TxEssence>(
    input: &TaikoInput<E>,
    header: &Header,
) -> Result<ProtocolInstance> {
    let tx_list_hash = TxHash::from(keccak::keccak(input.tx_list.as_slice()));
    let deposits = input
        .l2_input
        .withdrawals
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
                l1Hash: input.l1_hash,
                difficulty: header.difficulty.into(),
                txListHash: tx_list_hash,
                extraData: extra_data.into(),
                id: header.number,
                timestamp: header.timestamp.try_into().unwrap(),
                l1Height: input.l1_height,
                gasLimit: header.gas_limit.try_into().unwrap(),
                coinbase: header.beneficiary,
                depositsProcessed: deposits,
            },
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            signalRoot: input.signal_root,
            graffiti: input.graffiti,
        },
        prover: input.prover,
    };
    Ok(pi)
}
