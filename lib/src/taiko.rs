use crate::host::VerifyError;
use zeth_primitives::{
    block::Header,
    taiko::protocol_instance::ProtocolInstance,
    transactions::{ethereum::EthereumTxEssence, Transaction, TxEssence},
    B256, U256, U64,
};

// function anchor(
//   bytes32 l1BlockHash,
//   bytes32 l1SignalRoot,
//   uint64 l1Height,
//   uint32 parentGasUsed
// )

const CALL_START: usize = 4;
const EACH_PARAM_LEN: usize = 32;
const ANCHOR_GAS_LIMIT: u64 = 180_000;

#[allow(clippy::result_large_err)]
pub fn verify_anchor(
    anchor: &Transaction<EthereumTxEssence>,
    protocol_instance: &ProtocolInstance,
) -> Result<(), VerifyError> {
    if let EthereumTxEssence::Eip1559(ref tx) = anchor.essence {
        let mut start = CALL_START;
        let mut end = start + EACH_PARAM_LEN;
        let l1_block_hash = B256::from(&tx.data[start..end].try_into().unwrap());
        if l1_block_hash != protocol_instance.blockMetadata.l1Hash {
            return Err(VerifyError::AnchorCallDataMismatch);
        }
        start = end;
        end += EACH_PARAM_LEN;
        let _l1_signal_hash = B256::from(&tx.data[start..end].try_into().unwrap());

        start = end;
        end += EACH_PARAM_LEN;
        let l1_height =
            U256::from_be_bytes::<EACH_PARAM_LEN>(tx.data[start..end].try_into().unwrap());
        if U64::from(l1_height) != U64::from(protocol_instance.blockMetadata.l1Height) {
            return Err(VerifyError::AnchorCallDataMismatch);
        }
        start = end;
        end += EACH_PARAM_LEN;
        let _parent_gas_used =
            U256::from_be_bytes::<EACH_PARAM_LEN>(tx.data[start..end].try_into().unwrap());
        Ok(())
    } else {
        Err(VerifyError::AnchorTypeMisMatch {
            tx_type: anchor.essence.tx_type(),
        })
    }
}

#[allow(clippy::result_large_err)]
pub fn verify_block(
    block: &Header,
    protocol_instance: &ProtocolInstance,
) -> Result<(), VerifyError> {
    if block.difficulty != U256::try_from(protocol_instance.blockMetadata.difficulty).unwrap() {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.extra_data != protocol_instance.blockMetadata.extraData.to_vec() {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.timestamp != U256::from(protocol_instance.blockMetadata.timestamp) {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.gas_limit
        != U256::from(protocol_instance.blockMetadata.gasLimit) + U256::from(ANCHOR_GAS_LIMIT)
    {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.beneficiary != protocol_instance.blockMetadata.coinbase {
        return Err(VerifyError::BlockFieldMismatch);
    }
    match block.withdrawals_root {
        Some(withdraws_root) => {
            if withdraws_root != protocol_instance.blockMetadata.withdraws_root() {
                return Err(VerifyError::BlockFieldMismatch);
            }
        }
        None => todo!(),
    }
    if block.parent_hash != protocol_instance.parentHash {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.hash() != protocol_instance.blockHash {
        return Err(VerifyError::BlockFieldMismatch);
    }
    Ok(())
}
