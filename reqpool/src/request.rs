use crate::impl_display_using_json_pretty;
use alloy_primitives::Address;
use chrono::{DateTime, Utc};
use derive_getters::Getters;
use raiko_core::interfaces::{ProverSpecificOpts, ShastaProposalCheckpoint};
use raiko_lib::{
    input::BlobProofType,
    primitives::{ChainId, B256},
    proof_type::ProofType,
    prover::Proof,
};
use raiko_redis_derive::RedisValue;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::collections::HashMap;
use std::env;

#[derive(RedisValue, PartialEq, Debug, Clone, Deserialize, Serialize, Eq)]
#[serde(rename_all = "snake_case")]
/// The status of a request
pub enum Status {
    // === Normal status ===
    /// The request is registered but not yet started
    Registered,

    /// The request is in progress
    WorkInProgress,

    // /// The request is in progress of proving
    // WorkInProgressProving {
    //     /// The proof ID
    //     /// For SP1 and RISC0 proof type, it is the proof ID returned by the network prover,
    //     /// otherwise, it should be empty.
    //     proof_id: String,
    // },
    /// The request is successful
    Success {
        /// The proof of the request
        proof: Proof,
    },

    // === Cancelled status ===
    /// The request is cancelled
    Cancelled,

    // === Error status ===
    /// The request is failed with an error
    Failed {
        /// The error message
        error: String,
    },
}

impl Status {
    pub fn is_success(&self) -> bool {
        matches!(self, Status::Success { .. })
    }
}

#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, RedisValue, Getters,
)]
/// The status of a request with context
pub struct StatusWithContext {
    /// The status of the request
    status: Status,
    /// The timestamp of the status
    timestamp: DateTime<Utc>,
}

impl StatusWithContext {
    pub fn new(status: Status, timestamp: DateTime<Utc>) -> Self {
        Self { status, timestamp }
    }

    pub fn new_registered() -> Self {
        Self::new(Status::Registered, chrono::Utc::now())
    }

    pub fn new_cancelled() -> Self {
        Self::new(Status::Cancelled, chrono::Utc::now())
    }

    pub fn into_status(self) -> Status {
        self.status
    }
}

impl From<Status> for StatusWithContext {
    fn from(status: Status) -> Self {
        Self::new(status, chrono::Utc::now())
    }
}

/// The key to identify a request in the pool
#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue,
)]
pub enum RequestKey {
    GuestInput(GuestInputRequestKey),
    SingleProof(SingleProofRequestKey),
    Aggregation(AggregationRequestKey),
    BatchGuestInput(BatchGuestInputRequestKey),
    BatchProof(BatchProofRequestKey),
    ShastaGuestInput(ShastaInputRequestKey),
    ShastaProof(ShastaProofRequestKey),
    ShastaAggregation(AggregationRequestKey),
}

impl RequestKey {
    pub fn proof_type(&self) -> &ProofType {
        match self {
            RequestKey::GuestInput(_)
            | RequestKey::BatchGuestInput(_)
            | RequestKey::ShastaGuestInput(_) => &ProofType::Native,
            RequestKey::SingleProof(key) => &key.proof_type,
            RequestKey::Aggregation(key) => &key.proof_type,
            RequestKey::BatchProof(key) => &key.proof_type,
            RequestKey::ShastaProof(key) => &key.proof_type,
            RequestKey::ShastaAggregation(key) => &key.proof_type,
        }
    }
}

/// The key to identify a request in the pool
#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue, Getters,
)]
pub struct GuestInputRequestKey {
    /// The chain ID of the request
    chain_id: ChainId,
    /// The block number of the request
    block_number: u64,
    /// The block hash of the request
    block_hash: B256,
}

impl GuestInputRequestKey {
    pub fn new(chain_id: ChainId, block_number: u64, block_hash: B256) -> Self {
        Self {
            chain_id,
            block_number,
            block_hash,
        }
    }
}

/// The key to identify a request in the pool
#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue, Getters,
)]
pub struct SingleProofRequestKey {
    /// The chain ID of the request
    chain_id: ChainId,
    /// The block number of the request
    block_number: u64,
    /// The block hash of the request
    block_hash: B256,
    /// The proof type of the request
    proof_type: ProofType,
    /// The prover of the request
    prover_address: String,
    /// The image ID for zk provers (optional)
    image_id: Option<ImageId>,
}

impl SingleProofRequestKey {
    pub fn new(
        chain_id: ChainId,
        block_number: u64,
        block_hash: B256,
        proof_type: ProofType,
        prover_address: String,
    ) -> Self {
        Self {
            chain_id,
            block_number,
            block_hash,
            proof_type,
            prover_address,
            image_id: None,
        }
    }

    pub fn new_with_image_id(
        chain_id: ChainId,
        block_number: u64,
        block_hash: B256,
        proof_type: ProofType,
        prover_address: String,
        image_id: ImageId,
    ) -> Self {
        Self {
            chain_id,
            block_number,
            block_hash,
            proof_type,
            prover_address,
            image_id: Some(image_id.clone()),
        }
    }
}

#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue, Getters,
)]
/// The key to identify an aggregation request in the pool
pub struct AggregationRequestKey {
    // TODO add chain_id
    proof_type: ProofType,
    block_numbers: Vec<u64>,
    /// The image ID for zk provers (optional)
    image_id: Option<ImageId>,
}

impl AggregationRequestKey {
    pub fn new(proof_type: ProofType, block_numbers: Vec<u64>) -> Self {
        Self {
            proof_type,
            block_numbers,
            image_id: None,
        }
    }

    pub fn new_with_image_id(
        proof_type: ProofType,
        block_numbers: Vec<u64>,
        image_id: ImageId,
    ) -> Self {
        Self {
            proof_type,
            block_numbers,
            image_id: Some(image_id.clone()),
        }
    }
}

// The key to identify a batch guest input request in the pool
#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue, Getters,
)]
pub struct BatchGuestInputRequestKey {
    /// The chain ID of the request
    chain_id: ChainId,
    /// The block number of the request
    batch_id: u64,
    /// The l1 block number of the request
    l1_inclusion_height: u64,
}

impl BatchGuestInputRequestKey {
    pub fn new(chain_id: ChainId, batch_id: u64, l1_inclusion_height: u64) -> Self {
        Self {
            chain_id,
            batch_id,
            l1_inclusion_height,
        }
    }
}

/// The key to identify a request in the pool
#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue, Getters,
)]
pub struct BatchProofRequestKey {
    guest_input_key: BatchGuestInputRequestKey,
    /// The proof type of the request
    proof_type: ProofType,
    /// The prover of the request
    prover_address: String,
    /// The image ID for zk provers (optional)
    image_id: Option<ImageId>,
}

impl BatchProofRequestKey {
    pub fn new_with_input_key(
        guest_input_key: BatchGuestInputRequestKey,
        proof_type: ProofType,
        prover_address: String,
    ) -> Self {
        Self {
            guest_input_key,
            proof_type,
            prover_address,
            image_id: None,
        }
    }

    pub fn new_with_input_key_and_image_id(
        guest_input_key: BatchGuestInputRequestKey,
        proof_type: ProofType,
        prover_address: String,
        image_id: ImageId,
    ) -> Self {
        Self {
            guest_input_key,
            proof_type,
            prover_address,
            image_id: Some(image_id.clone()),
        }
    }

    pub fn new(
        chain_id: ChainId,
        batch_id: u64,
        l1_inclusion_height: u64,
        proof_type: ProofType,
        prover_address: String,
    ) -> Self {
        Self {
            guest_input_key: BatchGuestInputRequestKey::new(
                chain_id,
                batch_id,
                l1_inclusion_height,
            ),
            proof_type,
            prover_address,
            image_id: None,
        }
    }

    pub fn new_with_image_id(
        chain_id: ChainId,
        batch_id: u64,
        l1_inclusion_height: u64,
        proof_type: ProofType,
        prover_address: String,
        image_id: ImageId,
    ) -> Self {
        Self {
            guest_input_key: BatchGuestInputRequestKey::new(
                chain_id,
                batch_id,
                l1_inclusion_height,
            ),
            proof_type,
            prover_address,
            image_id: Some(image_id.clone()),
        }
    }
}

#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue, Getters,
)]
pub struct ShastaInputRequestKey {
    /// The proposal ID of the request
    proposal_id: u64,
    /// The L1 network of the request
    l1_network: String,
    /// The L2 network of the request
    l2_network: String,
}

impl ShastaInputRequestKey {
    pub fn new(proposal_id: u64, l1_network: String, l2_network: String) -> Self {
        Self {
            proposal_id,
            l1_network,
            l2_network,
        }
    }
}

#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue, Getters,
)]
pub struct ShastaProofRequestKey {
    guest_input_key: ShastaInputRequestKey,
    /// The proof type of the request
    proof_type: ProofType,
    /// The actual prover of the request (affects public input binding)
    actual_prover_address: String,
    /// The image ID for zk provers (optional)
    image_id: Option<ImageId>,
}

impl ShastaProofRequestKey {
    pub fn new_with_input_key(
        guest_input_key: ShastaInputRequestKey,
        proof_type: ProofType,
        actual_prover_address: String,
    ) -> Self {
        Self {
            guest_input_key,
            proof_type,
            actual_prover_address,
            image_id: None,
        }
    }

    pub fn new_with_input_key_and_image_id(
        guest_input_key: ShastaInputRequestKey,
        proof_type: ProofType,
        actual_prover_address: String,
        image_id: ImageId,
    ) -> Self {
        Self {
            guest_input_key,
            proof_type,
            actual_prover_address,
            image_id: Some(image_id),
        }
    }
}

impl From<GuestInputRequestKey> for RequestKey {
    fn from(key: GuestInputRequestKey) -> Self {
        RequestKey::GuestInput(key)
    }
}

impl From<SingleProofRequestKey> for RequestKey {
    fn from(key: SingleProofRequestKey) -> Self {
        RequestKey::SingleProof(key)
    }
}

impl From<AggregationRequestKey> for RequestKey {
    fn from(key: AggregationRequestKey) -> Self {
        RequestKey::Aggregation(key)
    }
}

impl From<BatchGuestInputRequestKey> for RequestKey {
    fn from(key: BatchGuestInputRequestKey) -> Self {
        RequestKey::BatchGuestInput(key)
    }
}

impl From<BatchProofRequestKey> for RequestKey {
    fn from(key: BatchProofRequestKey) -> Self {
        RequestKey::BatchProof(key)
    }
}

impl From<ShastaInputRequestKey> for RequestKey {
    fn from(key: ShastaInputRequestKey) -> Self {
        RequestKey::ShastaGuestInput(key)
    }
}

impl From<ShastaProofRequestKey> for RequestKey {
    fn from(key: ShastaProofRequestKey) -> Self {
        RequestKey::ShastaProof(key)
    }
}

// Helper functions to create request keys with image IDs
impl RequestKey {
    /// Create a SingleProof request key with image ID
    pub fn single_proof_with_image_id(
        chain_id: ChainId,
        block_number: u64,
        block_hash: B256,
        proof_type: ProofType,
        prover_address: String,
        image_id: ImageId,
    ) -> Self {
        RequestKey::SingleProof(SingleProofRequestKey::new_with_image_id(
            chain_id,
            block_number,
            block_hash,
            proof_type,
            prover_address,
            image_id,
        ))
    }

    /// Create an Aggregation request key with image ID
    pub fn aggregation_with_image_id(
        proof_type: ProofType,
        block_numbers: Vec<u64>,
        image_id: ImageId,
    ) -> Self {
        RequestKey::Aggregation(AggregationRequestKey::new_with_image_id(
            proof_type,
            block_numbers,
            image_id,
        ))
    }

    /// Create a BatchGuestInput request key without image ID
    pub fn batch_guest_input(chain_id: ChainId, batch_id: u64, l1_inclusion_height: u64) -> Self {
        RequestKey::BatchGuestInput(BatchGuestInputRequestKey::new(
            chain_id,
            batch_id,
            l1_inclusion_height,
        ))
    }

    /// Create a BatchProof request key with image ID
    pub fn batch_proof_with_image_id(
        chain_id: ChainId,
        batch_id: u64,
        l1_inclusion_height: u64,
        proof_type: ProofType,
        prover_address: String,
        image_id: ImageId,
    ) -> Self {
        RequestKey::BatchProof(BatchProofRequestKey::new_with_image_id(
            chain_id,
            batch_id,
            l1_inclusion_height,
            proof_type,
            prover_address,
            image_id,
        ))
    }

    /// Get the image ID from the request key if it exists
    pub fn image_id(&self) -> Option<&ImageId> {
        match self {
            RequestKey::GuestInput(_) => None, // GuestInput doesn't have image_id
            RequestKey::SingleProof(key) => key.image_id.as_ref(),
            RequestKey::Aggregation(key) => key.image_id.as_ref(),
            RequestKey::BatchProof(key) => key.image_id.as_ref(),
            RequestKey::BatchGuestInput(_) => None, // BatchGuestInput doesn't have image_id
            RequestKey::ShastaGuestInput(_) => None, // ShastaGuestInput doesn't have image_id
            RequestKey::ShastaProof(key) => key.image_id.as_ref(),
            RequestKey::ShastaAggregation(key) => key.image_id.as_ref(),
        }
    }
}

#[serde_as]
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue, Getters)]
pub struct GuestInputRequestEntity {
    /// The block number for the block to generate a proof for.
    block_number: u64,
    /// The l1 block number of the l2 block be proposed.
    l1_inclusion_block_number: u64,
    /// The network to generate the proof for.
    network: String,
    /// The L1 network to generate the proof for.
    l1_network: String,
    /// Graffiti.
    graffiti: B256,
    /// Blob proof type.
    blob_proof_type: BlobProofType,
    #[serde(flatten)]
    /// Additional prover params.
    prover_args: HashMap<String, serde_json::Value>,
}

impl GuestInputRequestEntity {
    pub fn new(
        block_number: u64,
        l1_inclusion_block_number: u64,
        network: String,
        l1_network: String,
        graffiti: B256,
        blob_proof_type: BlobProofType,
        prover_args: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            block_number,
            l1_inclusion_block_number,
            network,
            l1_network,
            graffiti,
            blob_proof_type,
            prover_args,
        }
    }
}

#[serde_as]
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue, Getters)]
pub struct SingleProofRequestEntity {
    /// The block number for the block to generate a proof for.
    block_number: u64,
    /// The l1 block number of the l2 block be proposed.
    l1_inclusion_block_number: u64,
    /// The network to generate the proof for.
    network: String,
    /// The L1 network to generate the proof for.
    l1_network: String,
    /// Graffiti.
    graffiti: B256,
    /// The protocol instance data.
    #[serde_as(as = "DisplayFromStr")]
    prover: Address,
    /// The proof type.
    proof_type: ProofType,
    /// Blob proof type.
    blob_proof_type: BlobProofType,
    #[serde(flatten)]
    /// Additional prover params.
    prover_args: HashMap<String, serde_json::Value>,
}

impl SingleProofRequestEntity {
    pub fn new(
        block_number: u64,
        l1_inclusion_block_number: u64,
        network: String,
        l1_network: String,
        graffiti: B256,
        prover: Address,
        proof_type: ProofType,
        blob_proof_type: BlobProofType,
        prover_args: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            block_number,
            l1_inclusion_block_number,
            network,
            l1_network,
            graffiti,
            prover,
            proof_type,
            blob_proof_type,
            prover_args,
        }
    }
}

#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue, Getters)]
pub struct AggregationRequestEntity {
    /// The block numbers and l1 inclusion block numbers for the blocks to aggregate proofs for.
    aggregation_ids: Vec<u64>,
    /// The block numbers and l1 inclusion block numbers for the blocks to aggregate proofs for.
    proofs: Vec<Proof>,
    /// The proof type.
    proof_type: ProofType,
    #[serde(flatten)]
    /// Any additional prover params in JSON format.
    prover_args: ProverSpecificOpts,
}

impl AggregationRequestEntity {
    pub fn new(
        aggregation_ids: Vec<u64>,
        proofs: Vec<Proof>,
        proof_type: ProofType,
        prover_args: ProverSpecificOpts,
    ) -> Self {
        Self {
            aggregation_ids,
            proofs,
            proof_type,
            prover_args,
        }
    }
}

#[serde_as]
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue, Getters)]
pub struct BatchGuestInputRequestEntity {
    /// The block number for the block to generate a proof for.
    batch_id: u64,
    /// The l1 block number of the l2 block be proposed.
    l1_inclusion_block_number: u64,
    /// The network to generate the proof for.
    network: String,
    /// The L1 network to generate the proof for.
    l1_network: String,
    /// Graffiti.
    graffiti: B256,
    /// Blob proof type.
    blob_proof_type: BlobProofType,
}

impl BatchGuestInputRequestEntity {
    pub fn new(
        batch_id: u64,
        l1_inclusion_block_number: u64,
        network: String,
        l1_network: String,
        graffiti: B256,
        blob_proof_type: BlobProofType,
    ) -> Self {
        Self {
            batch_id,
            l1_inclusion_block_number,
            network,
            l1_network,
            graffiti,
            blob_proof_type,
        }
    }
}

#[serde_as]
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue, Getters)]
pub struct ShastaInputRequestEntity {
    /// The block number for the block to generate a proof for.
    proposal_id: u64,
    /// The l1 block number of the l2 block be proposed.
    l1_inclusion_block_number: u64,
    /// The network to generate the proof for.
    network: String,
    /// The L1 network to generate the proof for.
    l1_network: String,
    /// actual prover
    actual_prover: Address,
    /// Blob proof type.
    blob_proof_type: BlobProofType,
    /// l2 blocks
    l2_blocks: Vec<u64>,
    /// checkpoint
    checkpoint: Option<ShastaProposalCheckpoint>,
    /// last anchor block number
    last_anchor_block_number: u64,
}

impl ShastaInputRequestEntity {
    pub fn new(
        proposal_id: u64,
        l1_inclusion_block_number: u64,
        network: String,
        l1_network: String,
        actual_prover: Address,
        blob_proof_type: BlobProofType,
        l2_blocks: Vec<u64>,
        checkpoint: Option<ShastaProposalCheckpoint>,
        last_anchor_block_number: u64,
    ) -> Self {
        Self {
            proposal_id,
            l1_inclusion_block_number,
            network,
            l1_network,
            actual_prover,
            blob_proof_type,
            l2_blocks,
            checkpoint,
            last_anchor_block_number,
        }
    }
}

#[serde_as]
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue, Getters)]
pub struct BatchProofRequestEntity {
    #[serde(flatten)]
    /// The batch input request entity
    guest_input_entity: BatchGuestInputRequestEntity,
    /// The protocol instance data.
    #[serde_as(as = "DisplayFromStr")]
    prover: Address,
    /// The proof type.
    proof_type: ProofType,
    #[serde(flatten)]
    /// Additional prover params.
    prover_args: HashMap<String, serde_json::Value>,
}

impl BatchProofRequestEntity {
    pub fn new(
        batch_id: u64,
        l1_inclusion_block_number: u64,
        network: String,
        l1_network: String,
        graffiti: B256,
        prover: Address,
        proof_type: ProofType,
        blob_proof_type: BlobProofType,
        prover_args: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            guest_input_entity: BatchGuestInputRequestEntity::new(
                batch_id,
                l1_inclusion_block_number,
                network,
                l1_network,
                graffiti,
                blob_proof_type,
            ),
            prover,
            proof_type,
            prover_args,
        }
    }

    pub fn new_with_guest_input_entity(
        guest_input_entity: BatchGuestInputRequestEntity,
        prover: Address,
        proof_type: ProofType,
        prover_args: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            guest_input_entity,
            prover,
            proof_type,
            prover_args,
        }
    }
}

#[serde_as]
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue, Getters)]
pub struct ShastaProofRequestEntity {
    #[serde(flatten)]
    /// The proposal input request entity
    guest_input_entity: ShastaInputRequestEntity,
    /// The proof type.
    proof_type: ProofType,
    #[serde(flatten)]
    /// Additional prover params.
    prover_args: HashMap<String, serde_json::Value>,
}

impl ShastaProofRequestEntity {
    pub fn new(
        batch_id: u64,
        l1_inclusion_block_number: u64,
        network: String,
        l1_network: String,
        actual_prover: Address,
        proof_type: ProofType,
        blob_proof_type: BlobProofType,
        l2_blocks: Vec<u64>,
        prover_args: HashMap<String, serde_json::Value>,
        checkpoint: Option<ShastaProposalCheckpoint>,
        last_anchor_block_number: u64,
    ) -> Self {
        Self {
            guest_input_entity: ShastaInputRequestEntity::new(
                batch_id,
                l1_inclusion_block_number,
                network,
                l1_network,
                actual_prover,
                blob_proof_type,
                l2_blocks,
                checkpoint,
                last_anchor_block_number,
            ),
            proof_type,
            prover_args,
        }
    }

    pub fn new_with_guest_input_entity(
        guest_input_entity: ShastaInputRequestEntity,
        proof_type: ProofType,
        prover_args: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            guest_input_entity,
            proof_type,
            prover_args,
        }
    }
}

/// The entity of a request
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue)]
pub enum RequestEntity {
    GuestInput(GuestInputRequestEntity),
    SingleProof(SingleProofRequestEntity),
    Aggregation(AggregationRequestEntity),
    BatchGuestInput(BatchGuestInputRequestEntity),
    BatchProof(BatchProofRequestEntity),
    ShastaGuestInput(ShastaInputRequestEntity),
    ShastaProof(ShastaProofRequestEntity),
    ShastaAggregation(AggregationRequestEntity),
}

impl From<GuestInputRequestEntity> for RequestEntity {
    fn from(entity: GuestInputRequestEntity) -> Self {
        RequestEntity::GuestInput(entity)
    }
}

impl From<SingleProofRequestEntity> for RequestEntity {
    fn from(entity: SingleProofRequestEntity) -> Self {
        RequestEntity::SingleProof(entity)
    }
}

impl From<AggregationRequestEntity> for RequestEntity {
    fn from(entity: AggregationRequestEntity) -> Self {
        // Pacaya and earlier forks still wrap AggregationRequestEntity in RequestEntity::Aggregation.
        // Shasta builds RequestEntity::ShastaAggregation explicitly, so this conversion is not used there.
        RequestEntity::Aggregation(entity)
    }
}

impl From<BatchGuestInputRequestEntity> for RequestEntity {
    fn from(entity: BatchGuestInputRequestEntity) -> Self {
        RequestEntity::BatchGuestInput(entity)
    }
}

impl From<ShastaInputRequestEntity> for RequestEntity {
    fn from(entity: ShastaInputRequestEntity) -> Self {
        RequestEntity::ShastaGuestInput(entity)
    }
}

impl From<BatchProofRequestEntity> for RequestEntity {
    fn from(entity: BatchProofRequestEntity) -> Self {
        RequestEntity::BatchProof(entity)
    }
}

impl From<ShastaProofRequestEntity> for RequestEntity {
    fn from(entity: ShastaProofRequestEntity) -> Self {
        RequestEntity::ShastaProof(entity)
    }
}

// === impl Display using json_pretty ===

impl_display_using_json_pretty!(RequestKey);
impl_display_using_json_pretty!(SingleProofRequestKey);
impl_display_using_json_pretty!(AggregationRequestKey);
impl_display_using_json_pretty!(BatchProofRequestKey);
impl_display_using_json_pretty!(RequestEntity);
impl_display_using_json_pretty!(SingleProofRequestEntity);
impl_display_using_json_pretty!(AggregationRequestEntity);
impl_display_using_json_pretty!(BatchProofRequestEntity);
impl_display_using_json_pretty!(BatchGuestInputRequestEntity);
impl_display_using_json_pretty!(ShastaInputRequestEntity);
impl_display_using_json_pretty!(ShastaProofRequestEntity);

// === impl Display for Status ===

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Registered => write!(f, "Registered"),
            Status::WorkInProgress => write!(f, "WorkInProgress"),
            Status::Success { .. } => write!(f, "Success"),
            Status::Cancelled => write!(f, "Cancelled"),
            Status::Failed { error } => write!(f, "Failed({})", error),
        }
    }
}

impl std::fmt::Display for StatusWithContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.status())
    }
}

/// Trait for reading image IDs for different proof types
pub trait ImageIdReader {
    /// Read the image ID from environment variables with fallback to default
    fn read_image_id(
        &self,
        request_type: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>>;

    /// Get the environment variable name for this proof type and request type
    fn env_var_name(&self, request_type: Option<&str>) -> &'static str;

    /// Get the default value if environment variable is not set
    fn default_value(&self, request_type: Option<&str>) -> &'static str;
}

impl ImageIdReader for ProofType {
    fn read_image_id(
        &self,
        request_type: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let env_var = self.env_var_name(request_type);
        match env::var(env_var) {
            Ok(value) => Ok(value),
            Err(_) => Ok(self.default_value(request_type).to_string()),
        }
    }

    fn env_var_name(&self, _request_type: Option<&str>) -> &'static str {
        match self {
            ProofType::Risc0 => "RISC0_BATCH_ID",
            ProofType::Sp1 => "SP1_BATCH_VK_HASH",
            ProofType::Sgx => "SGX_MRENCLAVE",
            ProofType::SgxGeth => "SGXGETH_MRENCLAVE",
            _ => panic!("Unsupported proof type for image ID: {:?}", self),
        }
    }

    fn default_value(&self, _request_type: Option<&str>) -> &'static str {
        match self {
            ProofType::Risc0 | ProofType::Sp1 | ProofType::Sgx | ProofType::SgxGeth => {
                "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            }
            _ => panic!("Unsupported proof type for default value: {:?}", self),
        }
    }
}

/// Image ID struct to hold different IDs for different proof types
#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue,
)]
pub struct ImageId {
    /// RISC0 batch ID
    pub risc0_batch_id: Option<String>,
    /// SP1 batch VK hash
    pub sp1_batch_vk_hash: Option<String>,
    /// SGX enclave MRENCLAVE
    pub sgx_enclave: Option<String>,
    /// SGX Geth enclave MRENCLAVE
    pub sgxgeth_enclave: Option<String>,
}

impl ImageId {
    pub fn new() -> Self {
        Self {
            risc0_batch_id: None,
            sp1_batch_vk_hash: None,
            sgx_enclave: None,
            sgxgeth_enclave: None,
        }
    }

    /// Create an ImageId based on the proof type (always uses batch IDs for data lookup)
    pub fn from_proof_type_and_request_type(proof_type: &ProofType, _is_aggregation: bool) -> Self {
        let mut image_id = Self::new();

        match proof_type {
            ProofType::Risc0 => {
                if let Ok(id) = proof_type.read_image_id(None) {
                    image_id.risc0_batch_id = Some(id);
                }
            }
            ProofType::Sp1 => {
                if let Ok(id) = proof_type.read_image_id(None) {
                    image_id.sp1_batch_vk_hash = Some(id);
                }
            }
            ProofType::Sgx => {
                if let Ok(mrenclave) = proof_type.read_image_id(None) {
                    image_id.sgx_enclave = Some(mrenclave);
                }
            }
            ProofType::SgxGeth => {
                if let Ok(mrenclave) = proof_type.read_image_id(None) {
                    image_id.sgxgeth_enclave = Some(mrenclave);
                }
            }
            _ => {
                // For proof type Native, we don't need image IDs
                // so we leave the ImageId empty
            }
        }

        image_id
    }
}

impl Default for ImageId {
    fn default() -> Self {
        Self::new()
    }
}

impl_display_using_json_pretty!(ImageId);
