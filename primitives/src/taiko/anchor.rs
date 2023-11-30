use alloy_primitives::B256;
use alloy_sol_types::{sol, Result, SolCall};

sol! {
    function anchor(
        bytes32 l1Hash,
        bytes32 l1SignalRoot,
        uint64 l1Height,
        uint32 parentGasUsed
    )
        external
    {}
}

/// decode anchor arguments from anchor transaction
pub fn decode_anchor_call_args(data: &[u8]) -> Result<(B256, B256, u64, u32)> {
    let decoded = anchorCall::abi_decode(data, false)?;
    Ok((
        decoded.l1Hash,
        decoded.l1SignalRoot,
        decoded.l1Height,
        decoded.parentGasUsed,
    ))
}
