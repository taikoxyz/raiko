use serde::{Deserialize, Serialize};

// Simple B256 type for block inputs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct B256([u8; 32]);

impl B256 {
    #[allow(dead_code)]
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[allow(dead_code)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// Simplified proof structure just for extracting input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    pub proof: Option<String>,
    pub input: Option<B256>,
    pub quote: Option<String>,
    pub uuid: Option<String>,
    pub kzg_proof: Option<String>,
}

// Simplified aggregation input just for extracting proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationGuestInput {
    pub proofs: Vec<Proof>,
}

// ZISK aggregation input structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkAggregationGuestInput {
    pub image_id: [u32; 8],
    pub block_inputs: Vec<B256>,
}