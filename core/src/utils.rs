use std::hint::black_box;

use reth_primitives::Header;
use serde_json::Value;
use tracing::error;

use crate::interfaces::{RaikoError, RaikoResult};

pub fn check_header(exp: &Header, header: &Header) -> Result<(), RaikoError> {
    // Check against the expected value of all fields for easy debugability
    check_eq(&exp.parent_hash, &header.parent_hash, "parent_hash");
    check_eq(&exp.ommers_hash, &header.ommers_hash, "ommers_hash");
    check_eq(&exp.beneficiary, &header.beneficiary, "beneficiary");
    check_eq(&exp.state_root, &header.state_root, "state_root");
    check_eq(
        &exp.transactions_root,
        &header.transactions_root,
        "transactions_root",
    );
    check_eq(&exp.receipts_root, &header.receipts_root, "receipts_root");
    check_eq(
        &exp.withdrawals_root,
        &header.withdrawals_root,
        "withdrawals_root",
    );
    check_eq(&exp.logs_bloom, &header.logs_bloom, "logs_bloom");
    check_eq(&exp.difficulty, &header.difficulty, "difficulty");
    check_eq(&exp.number, &header.number, "number");
    check_eq(&exp.gas_limit, &header.gas_limit, "gas_limit");
    check_eq(&exp.gas_used, &header.gas_used, "gas_used");
    check_eq(&exp.timestamp, &header.timestamp, "timestamp");
    check_eq(&exp.mix_hash, &header.mix_hash, "mix_hash");
    check_eq(&exp.nonce, &header.nonce, "nonce");
    check_eq(
        &exp.base_fee_per_gas,
        &header.base_fee_per_gas,
        "base_fee_per_gas",
    );
    check_eq(&exp.blob_gas_used, &header.blob_gas_used, "blob_gas_used");
    check_eq(
        &exp.excess_blob_gas,
        &header.excess_blob_gas,
        "excess_blob_gas",
    );
    check_eq(
        &exp.parent_beacon_block_root,
        &header.parent_beacon_block_root,
        "parent_beacon_block_root",
    );
    check_eq(&exp.extra_data, &header.extra_data, "extra_data");

    // Make sure the blockhash from the node matches the one from the builder
    require_eq(
        &exp.hash_slow(),
        &header.hash_slow(),
        &format!("block hash unexpected for block {}", exp.number),
    )
}

pub fn check_eq<T: std::cmp::PartialEq + std::fmt::Debug>(expected: &T, actual: &T, message: &str) {
    // printing out error, if any, but ignoring the result
    // making sure it's not optimized out
    let _ = black_box(require_eq(expected, actual, message));
}

pub fn require(expression: bool, message: &str) -> RaikoResult<()> {
    if !expression {
        let msg = format!("Assertion failed: {message}");
        error!("{msg}");
        return Err(anyhow::Error::msg(msg).into());
    }
    Ok(())
}

pub fn require_eq<T: std::cmp::PartialEq + std::fmt::Debug>(
    expected: &T,
    actual: &T,
    message: &str,
) -> RaikoResult<()> {
    let msg = format!("{message} - Expected: {expected:?}, Found: {actual:?}");
    require(expected == actual, &msg)
}

/// Merges two json's together, overwriting `a` with the values of `b`
pub fn merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (a, b) if !b.is_null() => b.clone_into(a),
        // If b is null, just keep a (which means do nothing).
        _ => {}
    }
}
