//! Output types for raiko2 guest programs.

use alloy_primitives::B256;
use reth_ethereum_primitives::Block;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

/// Guest output for a single block.
#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestOutput {
    pub header: alloy_consensus::Header,
    pub hash: B256,
}

/// Guest batch output for multiple blocks.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GuestBatchOutput {
    pub blocks: Vec<Block>,
    pub hash: B256,
}

/// Aggregation guest output.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AggregationGuestOutput {
    /// The resulting hash.
    pub hash: B256,
}
