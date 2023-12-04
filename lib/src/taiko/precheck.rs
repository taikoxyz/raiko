use std::error::Error;

use anyhow::{bail, Context, Result};
use ethers_core::types::{Block, EIP1186ProofResponse, Transaction, H160, U256, U64};
use zeth_primitives::{
    ethers::{from_ethers_h160, from_ethers_u256},
    signature::TxSignature,
    taiko::{ANCHOR_GAS_LIMIT, GOLDEN_TOUCH_ACCOUNT, GX1, GX2, L2_CONTRACT},
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        EthereumTransaction, TxEssence,
    },
    Address,
};

use crate::taiko::{host::TaikoExtra, utils::rlp_decode_list};

// rebuild the block with anchor transaction and txlist from l1 contract, then precheck it
pub fn rebuild_and_precheck_block(
    l2_fini: &mut Block<Transaction>,
    extra: &TaikoExtra,
) -> Result<()> {
    let Some(anchor) = l2_fini.transactions.first().cloned() else {
        bail!("no anchor transaction found");
    };
    // 1. check anchor transaction
    precheck_anchor(l2_fini, &anchor).context("precheck anchor error")?;

    // 2. patch anchor transaction into tx list instead of those from l2 node's
    let mut txs: Vec<Transaction> =
        rlp_decode_list(&extra.l2_tx_list).context("failed to decode tx list")?;
    // insert the anchor transaction into the tx list at the first position
    txs.insert(0, anchor.clone());
    // reset transactions
    l2_fini.transactions = txs;
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
    AnchorFeeCapMisMatch {
        expected: Option<U256>,
        got: Option<U256>,
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

pub fn precheck_anchor(
    l2_fini: &Block<Transaction>,
    anchor: &Transaction,
) -> Result<(), AnchorError> {
    let tx1559_type = U64::from(0x2);
    if !matches!(anchor.transaction_type, Some(tx1559_type)) {
        return Err(AnchorError::AnchorTypeMisMatch {
            tx_type: anchor.transaction_type.unwrap_or_default().as_u64() as u8,
        });
    }
    // verify transaction
    precheck_anchor_signature(&TxSignature {
        v: anchor.v.as_u64(),
        r: from_ethers_u256(anchor.r),
        s: from_ethers_u256(anchor.s),
    })?;
    // verify the transaction signature
    let from = from_ethers_h160(anchor.from);
    if from != *GOLDEN_TOUCH_ACCOUNT {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: *GOLDEN_TOUCH_ACCOUNT,
            got: Some(from),
        });
    }
    let Some(to) = anchor.to else {
        return Err(AnchorError::AnchorToMisMatch {
            expected: *L2_CONTRACT,
            got: None,
        });
    };
    let to = from_ethers_h160(to);
    if to != *L2_CONTRACT {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: *L2_CONTRACT,
            got: Some(to),
        });
    }
    if anchor.value != U256::zero() {
        return Err(AnchorError::AnchorValueMisMatch {
            expected: U256::zero(),
            got: anchor.value,
        });
    }
    if anchor.gas != U256::from(ANCHOR_GAS_LIMIT) {
        return Err(AnchorError::AnchorGasLimitMisMatch {
            expected: U256::from(ANCHOR_GAS_LIMIT),
            got: anchor.gas,
        });
    }
    // anchor's gas price should be the same as the block's
    if anchor.max_fee_per_gas != l2_fini.base_fee_per_gas {
        return Err(AnchorError::AnchorFeeCapMisMatch {
            expected: l2_fini.base_fee_per_gas,
            got: anchor.max_fee_per_gas,
        });
    }
    Ok(())
}
