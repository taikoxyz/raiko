use p3_challenger::DuplexChallenger;
use serde::{Deserialize, Serialize};

use sp1_core::{
    air::PublicValues,
    runtime::ExecutionState,
    utils::baby_bear_poseidon2::{Perm, Val},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialProofRequest {
    pub checkpoint_id: usize,
    pub checkpoint_data: ExecutionState,
    pub challenger: DuplexChallenger<Val, Perm, 16, 8>,
    pub public_values: PublicValues<u32, u32>,
    pub shard_batch_size: usize,
}
