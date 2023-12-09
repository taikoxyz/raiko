use std::error::Error;

use anyhow::{bail, Context, Result};
use ethers_core::types::{Block, Transaction as EthersTransaction, U256 as EthersU256, U64};
use once_cell::sync::Lazy;
use zeth_primitives::{
    ethers::{from_ethers_h160, from_ethers_u256},
    signature::TxSignature,
    taiko::{ANCHOR_GAS_LIMIT, GOLDEN_TOUCH_ACCOUNT, L2_CONTRACT},
    transactions::ethereum::EthereumTxEssence,
    uint, Address, B256, U256,
};

use crate::taiko::{host::TaikoExtra, utils::rlp_decode_list};

static GX1: Lazy<U256> =
    Lazy::new(|| uint!(0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798_U256));
static N: Lazy<U256> =
    Lazy::new(|| uint!(0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141_U256));
static GX1_MUL_PRIVATEKEY: Lazy<U256> =
    Lazy::new(|| uint!(0x4341adf5a780b4a87939938fd7a032f6e6664c7da553c121d3b4947429639122_U256));
static GX2: Lazy<U256> =
    Lazy::new(|| uint!(0xc6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5_U256));

// rebuild the block with anchor transaction and txlist from l1 contract, then precheck it
pub fn rebuild_and_precheck_block(
    l2_fini: &mut Block<EthersTransaction>,
    extra: &TaikoExtra,
) -> Result<()> {
    let Some(anchor) = l2_fini.transactions.first().cloned() else {
        bail!("no anchor transaction found");
    };
    // 1. check anchor transaction
    precheck_anchor(l2_fini, &anchor).context("precheck anchor error")?;

    // 2. patch anchor transaction into tx list instead of those from l2 node's
    let mut txs: Vec<EthersTransaction> =
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
        expected: u8,
        got: u8,
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
        expected: EthersU256,
        got: EthersU256,
    },
    AnchorGasLimitMisMatch {
        expected: EthersU256,
        got: EthersU256,
    },
    AnchorFeeCapMisMatch {
        expected: Option<EthersU256>,
        got: Option<EthersU256>,
    },
    AnchorSignatureMismatch {
        msg: String,
    },
    Anyhow(anyhow::Error),
}

impl From<anyhow::Error> for AnchorError {
    fn from(e: anyhow::Error) -> Self {
        AnchorError::Anyhow(e)
    }
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#?}", self)
    }
}

impl Error for AnchorError {}

fn precheck_anchor_signature(sign: &TxSignature, msg_hash: B256) -> Result<(), AnchorError> {
    if sign.r == *GX1 {
        return Ok(());
    }
    let msg_hash: U256 = msg_hash.into();
    if sign.r == *GX2 {
        if *N != msg_hash + *GX1_MUL_PRIVATEKEY {
            return Err(AnchorError::AnchorSignatureMismatch {
                msg: format!(
                    "r == GX2, but N != msg_hash + GX1_MUL_PRIVATEKEY, N: {}, msg_hash: {}, GX1_MUL_PRIVATEKEY: {}",
                    *N, msg_hash, *GX1_MUL_PRIVATEKEY
                ),
            });
        }
        return Ok(());
    }
    Err(AnchorError::AnchorSignatureMismatch {
        msg: format!(
            "r != GX1 && r != GX2, r: {}, GX1: {}, GX2: {}",
            sign.r, *GX1, *GX2
        ),
    })
}

pub fn precheck_anchor(
    l2_fini: &Block<EthersTransaction>,
    anchor: &EthersTransaction,
) -> Result<(), AnchorError> {
    let tx1559_type = U64::from(0x2);
    if anchor.transaction_type != Some(tx1559_type) {
        return Err(AnchorError::AnchorTypeMisMatch {
            expected: tx1559_type.as_u64() as u8,
            got: anchor.transaction_type.unwrap_or_default().as_u64() as u8,
        });
    }
    let tx: EthereumTxEssence = anchor.clone().try_into()?;
    // verify transaction
    precheck_anchor_signature(
        &TxSignature {
            v: anchor.v.as_u64(),
            r: from_ethers_u256(anchor.r),
            s: from_ethers_u256(anchor.s),
        },
        tx.signing_hash(),
    )?;
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
    if anchor.value != EthersU256::zero() {
        return Err(AnchorError::AnchorValueMisMatch {
            expected: EthersU256::zero(),
            got: anchor.value,
        });
    }
    if anchor.gas != EthersU256::from(ANCHOR_GAS_LIMIT) {
        return Err(AnchorError::AnchorGasLimitMisMatch {
            expected: EthersU256::from(ANCHOR_GAS_LIMIT),
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
