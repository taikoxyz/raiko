use std::str::FromStr;

use once_cell::sync::Lazy;
use zeth_primitives::{
    block::Header,
    taiko::{
        protocol_instance::ProtocolInstance, ANCHOR_GAS_LIMIT, ANCHOR_SELECTOR,
        GOLDEN_TOUCH_ACCOUNT, L2_CONTRACT,
    },
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        Transaction, TxEssence,
    },
    Address, B256, U256, U64,
};

use crate::host::{AnchorError, VerifyError};

pub const CALL_START: usize = 4;
pub const EACH_PARAM_LEN: usize = 32;

#[allow(clippy::result_large_err)]
pub fn verify_anchor(
    block: &Header,
    anchor: &Transaction<EthereumTxEssence>,
    protocol_instance: &ProtocolInstance,
) -> Result<(), VerifyError> {
    if let EthereumTxEssence::Eip1559(ref tx) = anchor.essence {
        // verify transaction
        // verify the transaction signature
        match anchor.recover_from() {
            Ok(from) => {
                if from != *GOLDEN_TOUCH_ACCOUNT {
                    return Err(AnchorError::AnchorFromMisMatch {
                        expected: *L2_CONTRACT,
                        got: Some(from),
                    }
                    .into());
                }
            }
            Err(_) => {
                return Err(AnchorError::AnchorToMisMatch {
                    expected: *L2_CONTRACT,
                    got: None,
                }
                .into())
            }
        }

        match tx.to {
            TransactionKind::Call(to) => {
                if to != *L2_CONTRACT {
                    return Err(AnchorError::AnchorToMisMatch {
                        expected: *L2_CONTRACT,
                        got: Some(to),
                    }
                    .into());
                }
            }
            _ => {
                return Err(AnchorError::AnchorToMisMatch {
                    expected: *L2_CONTRACT,
                    got: None,
                }
                .into())
            }
        }
        if tx.value != U256::ZERO {
            return Err(AnchorError::AnchorValueMisMatch {
                expected: U256::ZERO,
                got: tx.value,
            }
            .into());
        }
        if tx.gas_limit != U256::from(ANCHOR_GAS_LIMIT) {
            return Err(AnchorError::AnchorGasLimitMisMatch {
                expected: U256::from(ANCHOR_GAS_LIMIT),
                got: tx.gas_limit,
            }
            .into());
        }
        if tx.max_fee_per_gas != block.base_fee_per_gas {
            return Err(AnchorError::AnchorGasPriceMisMatch {
                expected: U256::from(ANCHOR_GAS_LIMIT),
                got: tx.gas_limit,
            }
            .into());
        }
        // verify calldata
        let selector: [u8; 4] = tx.data[..CALL_START].try_into().unwrap();
        if selector != ANCHOR_SELECTOR {
            return Err(AnchorError::AnchorCallDataMismatch.into());
        }
        let mut start = CALL_START;
        let mut end = start + EACH_PARAM_LEN;
        let l1_block_hash = B256::from(&tx.data[start..end].try_into().unwrap());
        if l1_block_hash != protocol_instance.block_evidence.blockMetadata.l1Hash {
            return Err(AnchorError::AnchorCallDataMismatch.into());
        }
        start = end;
        end += EACH_PARAM_LEN;
        // TODO: l1 signal root verify
        let _l1_signal_hash = B256::from(&tx.data[start..end].try_into().unwrap());

        start = end;
        end += EACH_PARAM_LEN;
        let l1_height =
            U256::from_be_bytes::<EACH_PARAM_LEN>(tx.data[start..end].try_into().unwrap());
        if U64::from(l1_height)
            != U64::from(protocol_instance.block_evidence.blockMetadata.l1Height)
        {
            return Err(AnchorError::AnchorCallDataMismatch.into());
        }
        start = end;
        end += EACH_PARAM_LEN;
        // TODO: Get the l2 block parent gas used
        let _parent_gas_used =
            U256::from_be_bytes::<EACH_PARAM_LEN>(tx.data[start..end].try_into().unwrap());
        Ok(())
    } else {
        Err(AnchorError::AnchorTypeMisMatch {
            tx_type: anchor.essence.tx_type(),
        }
        .into())
    }
}

#[allow(clippy::result_large_err)]
pub fn verify_block(
    block: &Header,
    protocol_instance: &ProtocolInstance,
) -> Result<(), VerifyError> {
    if block.difficulty
        != U256::try_from(protocol_instance.block_evidence.blockMetadata.difficulty).unwrap()
    {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.extra_data
        != protocol_instance
            .block_evidence
            .blockMetadata
            .extraData
            .to_vec()
    {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.timestamp != U256::from(protocol_instance.block_evidence.blockMetadata.timestamp) {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.gas_limit
        != U256::from(protocol_instance.block_evidence.blockMetadata.gasLimit)
            + U256::from(ANCHOR_GAS_LIMIT)
    {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.beneficiary != protocol_instance.block_evidence.blockMetadata.coinbase {
        return Err(VerifyError::BlockFieldMismatch);
    }
    match block.withdrawals_root {
        Some(_withdraws_root) => {
            // TODO: verify withdraws root
            // if withdraws_root != protocol_instance.blockMetadata.withdraws_root() {
            //     return Err(VerifyError::BlockFieldMismatch);
            // }
        }
        None => todo!(),
    }
    if block.parent_hash != protocol_instance.block_evidence.parentHash {
        return Err(VerifyError::BlockFieldMismatch);
    }
    if block.hash() != protocol_instance.block_evidence.blockHash {
        return Err(VerifyError::BlockFieldMismatch);
    }
    Ok(())
}
