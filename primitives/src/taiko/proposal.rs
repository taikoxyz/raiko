use alloy_sol_types::{sol, SolType, SolCall};
use anyhow::{Context, Result};

sol! {
    struct HookCall {
        address hook;
        bytes data;
    }

    struct BlockParams {
        address assignedProver;
        address coinbase;
        bytes32 extraData;
        bytes32 blobHash;
        uint24 txListByteOffset;
        uint24 txListByteSize;
        bool cacheBlobForReuse;
        bytes32 parentMetaHash;
        HookCall[] hookCalls;
    }

    function proposeBlock(
        bytes calldata params,
        bytes calldata txList
    )
    {}
}

pub fn decode_propose_block_call_args(data: &[u8]) -> Result<proposeBlockCall> {
    let propose_block_call = proposeBlockCall::abi_decode(data, false)
        .with_context(|| "failed to decode propose block call")?;
    Ok(propose_block_call)
}

pub fn decode_propose_block_call_params(data: &[u8]) -> Result<BlockParams> {
    let propose_block_params = BlockParams::abi_decode(data, false)
    .with_context(|| "failed to decode propose block call")?;
    Ok(propose_block_params)
}