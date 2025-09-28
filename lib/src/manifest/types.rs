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


/*
 /// @notice Represents a block manifest
    struct BlockManifest {
        /// @notice The timestamp of the block.
        uint48 timestamp;
        /// @notice The coinbase of the block.
        address coinbase;
        /// @notice The anchor block number. This field can be zero, if so, this block will use the
        /// most recent anchor in a previous block.
        uint48 anchorBlockNumber;
        /// @notice The block's gas limit.
        uint48 gasLimit;
        /// @notice The transactions for this block.
        SignedTransaction[] transactions;
    }
*/
/// Block manifest with extra information - corresponds to Go's BlockManifest
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockManifest {
    /// Protocol block manifest
    #[serde(flatten)]
    pub protocol: ProtocolBlockManifest,
    /// Bond instructions hash
    pub bond_instructions_hash: alloy_primitives::B256,
    /// Bond instructions
    pub bond_instructions: Vec<BondInstruction>,
}

/// Protocol proposal manifest - corresponds to Go's ProtocolProposalManifest
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolProposalManifest {
    /// Prover authentication bytes
    pub prover_auth_bytes: Bytes,
    /// Blocks in this proposal
    pub blocks: Vec<ProtocolBlockManifest>,
}

/// Proposal manifest with extra information - corresponds to Go's ProposalManifest
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProposalManifest {
    /// Prover authentication bytes
    pub prover_auth_bytes: Bytes,
    /// Blocks in this proposal
    pub blocks: Vec<BlockManifest>,
    /// Whether this is a default proposal
    pub default: bool,
    /// Parent block (optional)
    pub parent_block: Option<reth_primitives::Block>,
    /// Whether this is a low bond proposal
    pub is_low_bond_proposal: bool,
}
