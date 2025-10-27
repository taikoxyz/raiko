// code logic comes from: packages/protocol/contracts/layer1/shasta/libs/LibManifest.sol

use alloy_primitives::{Address, Bytes, U256};
use reth_primitives::TransactionSigned;
use serde::{Deserialize, Serialize};

/// Protocol block manifest - corresponds to Go's ProtocolBlockManifest
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolBlockManifest {
    /// The timestamp of the block
    pub timestamp: u64,
    /// The coinbase of the block
    pub coinbase: Address,
    /// The anchor block number. This field can be zero, if so, this block will use the
    /// most recent anchor in a previous block
    pub anchor_block_number: u64,
    /// The block's gas limit
    pub gas_limit: u64,
    /// The transactions for this block
    pub transactions: Vec<TransactionSigned>,
}

/// Bond instruction - corresponds to Go's LibBondsBondInstruction
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondInstruction {
    pub proposal_id: U256,
    pub bond_type: u8,
    pub payer: Address,
    pub receiver: Address,
}

/// Protocol proposal manifest - corresponds to Go's ProtocolProposalManifest
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DerivationSourceManifest {
    /// blocks in this source
    pub blocks: Vec<ProtocolBlockManifest>,
}

/// Protocol proposal manifest - corresponds to Go's ProtocolProposalManifest
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolProposalManifest {
    /// Prover authentication bytes
    pub prover_auth_bytes: Bytes,
    /// Sources in this proposal
    pub sources: Vec<DerivationSourceManifest>,
}
