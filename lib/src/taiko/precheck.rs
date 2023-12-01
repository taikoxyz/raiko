use std::error::Error;

use anyhow::{bail, Context, Result};
use ethers_core::types::Transaction as EthersTransaction;
use zeth_primitives::{
    signature::TxSignature,
    taiko::{ANCHOR_GAS_LIMIT, GOLDEN_TOUCH_ACCOUNT, GX1, GX2, L2_CONTRACT},
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        EthereumTransaction, TxEssence,
    },
    Address, U256,
};

use crate::taiko::{host::TaikoInit, utils::rlp_decode_list};

pub fn precheck_block(init: &mut TaikoInit<EthereumTxEssence>) -> Result<()> {
    let Some(anchor) = init.l2_init.fini_transactions.first().cloned() else {
        bail!("no anchor transaction found");
    };
    // 1. check anchor transaction
    precheck_anchor(init, &anchor).context("precheck anchor error")?;

    // 2. patch anchor transaction into tx list instead of those from l2 node's
    let remaining_txs: Vec<EthersTransaction> =
        rlp_decode_list(&init.tx_list).context("failed to decode tx list")?;
    let mut txs: Vec<EthereumTransaction> = remaining_txs
        .into_iter()
        .map(|tx| tx.try_into().unwrap())
        .collect();
    txs.insert(0, anchor.clone());
    init.l2_init.fini_transactions = txs;
    Ok(())
}

#[derive(Debug)]
pub enum AnchorError {
    AnchorTypeMisMatch {
        tx_type: u8,
    },
    AnchorFromMisMatch {
        expected: Address,
        got: Option<Address>,
    },
    AnchorToMisMatch {
        expected: Address,
        got: Option<Address>,
    },
    AnchorValueMisMatch {
        expected: U256,
        got: U256,
    },
    AnchorGasLimitMisMatch {
        expected: U256,
        got: U256,
    },
    AnchorGasPriceMisMatch {
        expected: U256,
        got: U256,
    },
    AnchorSignatureMismatch,
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#?}", self)
    }
}

impl Error for AnchorError {}

fn precheck_anchor_signature(sign: &TxSignature) -> Result<(), AnchorError> {
    if sign.r == *GX1 || sign.r == *GX2 {
        // TODO: when r == GX2 require s == 0 if k == 1
        return Ok(());
    }
    return Err(AnchorError::AnchorSignatureMismatch);
}

fn precheck_anchor(
    init: &TaikoInit<EthereumTxEssence>,
    anchor: &EthereumTransaction,
) -> Result<(), AnchorError> {
    let EthereumTxEssence::Eip1559(tx) = &anchor.essence else {
        return Err(AnchorError::AnchorTypeMisMatch {
            tx_type: anchor.essence.tx_type(),
        });
    };
    // verify transaction
    precheck_anchor_signature(&anchor.signature)?;
    // verify the transaction signature
    let Ok(from) = anchor.recover_from() else {
        return Err(AnchorError::AnchorToMisMatch {
            expected: *L2_CONTRACT,
            got: None,
        });
    };
    if from != *GOLDEN_TOUCH_ACCOUNT {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: *GOLDEN_TOUCH_ACCOUNT,
            got: Some(from),
        });
    }
    let TransactionKind::Call(to) = tx.to else {
        return Err(AnchorError::AnchorToMisMatch {
            expected: *L2_CONTRACT,
            got: None,
        });
    };
    if to != *L2_CONTRACT {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: *L2_CONTRACT,
            got: Some(from),
        });
    }
    if tx.value != U256::ZERO {
        return Err(AnchorError::AnchorValueMisMatch {
            expected: U256::ZERO,
            got: tx.value,
        });
    }
    if tx.gas_limit != U256::from(ANCHOR_GAS_LIMIT) {
        return Err(AnchorError::AnchorGasLimitMisMatch {
            expected: U256::from(ANCHOR_GAS_LIMIT),
            got: tx.gas_limit,
        });
    }
    if tx.max_fee_per_gas != init.l2_init.fini_block.base_fee_per_gas {
        return Err(AnchorError::AnchorGasPriceMisMatch {
            expected: U256::from(ANCHOR_GAS_LIMIT),
            got: anchor.essence.gas_limit(),
        });
    }
    Ok(())
}
