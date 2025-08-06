use crate::impl_display_using_json_pretty;
use alloy_primitives::Address;
use chrono::{DateTime, Utc};
use derive_getters::Getters;
use raiko_core::interfaces::ProverSpecificOpts;
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

#[derive(RedisValue, PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord)]
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
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, RedisValue, Getters,
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
}

impl RequestKey {
    pub fn proof_type(&self) -> &ProofType {
        match self {
            RequestKey::GuestInput(_) | RequestKey::BatchGuestInput(_) => &ProofType::Native,
            RequestKey::SingleProof(key) => &key.proof_type,
            RequestKey::Aggregation(key) => &key.proof_type,
            RequestKey::BatchProof(key) => &key.proof_type,
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
    /// The image ID for zk provers (optional)
    image_id: Option<ImageId>,
}

impl GuestInputRequestKey {
    pub fn new(chain_id: ChainId, block_number: u64, block_hash: B256) -> Self {
        Self {
            chain_id,
            block_number,
            block_hash,
            image_id: None,
        }
    }

    pub fn new_with_image_id(
        chain_id: ChainId,
        block_number: u64,
        block_hash: B256,
        image_id: ImageId,
    ) -> Self {
        Self {
            chain_id,
            block_number,
            block_hash,
            image_id: Some(image_id),
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
    /// The image ID for zk provers (optional)
    image_id: Option<ImageId>,
}

impl BatchGuestInputRequestKey {
    pub fn new(chain_id: ChainId, batch_id: u64, l1_inclusion_height: u64) -> Self {
        Self {
            chain_id,
            batch_id,
            l1_inclusion_height,
            image_id: None,
        }
    }

    pub fn new_with_image_id(
        chain_id: ChainId,
        batch_id: u64,
        l1_inclusion_height: u64,
        image_id: ImageId,
    ) -> Self {
        Self {
            chain_id,
            batch_id,
            l1_inclusion_height,
            image_id: Some(image_id.clone()),
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
            guest_input_key: BatchGuestInputRequestKey::new_with_image_id(
                chain_id,
                batch_id,
                l1_inclusion_height,
                image_id.clone(),
            ),
            proof_type,
            prover_address,
            image_id: Some(image_id.clone()),
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

// Helper functions to create request keys with image IDs
impl RequestKey {
    /// Create a GuestInput request key with image ID
    pub fn guest_input_with_image_id(
        chain_id: ChainId,
        block_number: u64,
        block_hash: B256,
        image_id: ImageId,
    ) -> Self {
        RequestKey::GuestInput(GuestInputRequestKey::new_with_image_id(
            chain_id,
            block_number,
            block_hash,
            image_id,
        ))
    }

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

    /// Create a BatchGuestInput request key with image ID
    pub fn batch_guest_input_with_image_id(
        chain_id: ChainId,
        batch_id: u64,
        l1_inclusion_height: u64,
        image_id: ImageId,
    ) -> Self {
        RequestKey::BatchGuestInput(BatchGuestInputRequestKey::new_with_image_id(
            chain_id,
            batch_id,
            l1_inclusion_height,
            image_id,
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
            RequestKey::GuestInput(key) => key.image_id.as_ref(),
            RequestKey::SingleProof(key) => key.image_id.as_ref(),
            RequestKey::Aggregation(key) => key.image_id.as_ref(),
            RequestKey::BatchGuestInput(key) => key.image_id.as_ref(),
            RequestKey::BatchProof(key) => key.image_id.as_ref(),
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

/// The entity of a request
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue)]
pub enum RequestEntity {
    GuestInput(GuestInputRequestEntity),
    SingleProof(SingleProofRequestEntity),
    Aggregation(AggregationRequestEntity),
    BatchGuestInput(BatchGuestInputRequestEntity),
    BatchProof(BatchProofRequestEntity),
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
        RequestEntity::Aggregation(entity)
    }
}

impl From<BatchGuestInputRequestEntity> for RequestEntity {
    fn from(entity: BatchGuestInputRequestEntity) -> Self {
        RequestEntity::BatchGuestInput(entity)
    }
}

impl From<BatchProofRequestEntity> for RequestEntity {
    fn from(entity: BatchProofRequestEntity) -> Self {
        RequestEntity::BatchProof(entity)
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

/// Image ID struct to hold different IDs for different proof types
#[derive(
    PartialEq, Debug, Clone, Deserialize, Serialize, Eq, PartialOrd, Ord, Hash, RedisValue,
)]
pub struct ImageId {
    /// RISC0 aggregation ID
    pub risc0_agg_id: Option<String>,
    /// RISC0 batch ID
    pub risc0_batch_id: Option<String>,
    /// SP1 aggregation VK hash
    pub sp1_agg_vk_hash: Option<String>,
    /// SP1 batch VK hash
    pub sp1_batch_vk_hash: Option<String>,
}

impl ImageId {
    pub fn new() -> Self {
        Self {
            risc0_agg_id: None,
            risc0_batch_id: None,
            sp1_agg_vk_hash: None,
            sp1_batch_vk_hash: None,
        }
    }

    /// Read RISC0 ID (aggregation or batch) from environment variables
    pub fn read_risc0_id(id_type: &str) -> Result<String, Box<dyn std::error::Error>> {
        let env_var = match id_type {
            "aggregation" => "RISC0_AGGREGATION_ID",
            "batch" => "RISC0_BATCH_ID",
            _ => return Err("Invalid RISC0 ID type. Must be 'aggregation' or 'batch'".into()),
        };

        match env::var(env_var) {
            Ok(value) => Ok(value),
            Err(_) => {
                // Fallback to default hardcoded values if env var is not set
                let default_hex = match id_type {
                    "aggregation" => {
                        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    }
                    "batch" => {
                        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    }
                    _ => unreachable!(),
                };

                Ok(default_hex.to_string())
            }
        }
    }

    /// Read SP1 VK hash (aggregation or batch) from environment variables
    pub fn read_sp1_vk_hash(hash_type: &str) -> Result<String, Box<dyn std::error::Error>> {
        let env_var = match hash_type {
            "aggregation" => "SP1_AGGREGATION_VK_HASH",
            "batch" => "SP1_BATCH_VK_HASH",
            _ => return Err("Invalid SP1 hash type. Must be 'aggregation' or 'batch'".into()),
        };

        match env::var(env_var) {
            Ok(value) => Ok(value),
            Err(_) => {
                // Fallback to default hardcoded values if env var is not set.
                // SP1 generates two image IDs per binary (aggregation or batch): vk_hash and vk_bn256.
                // We use only vk_hash here, which is sufficient to verify the binary version.
                let default_hash = match hash_type {
                    "aggregation" => {
                        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    }
                    "batch" => {
                        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    }
                    _ => unreachable!(),
                };
                Ok(default_hash.to_string())
            }
        }
    }

    /// Create an ImageId based on the proof type and request type (aggregation or batch)
    pub fn from_proof_type_and_request_type(proof_type: &ProofType, is_aggregation: bool) -> Self {
        let mut image_id = Self::new();

        match proof_type {
            ProofType::Risc0 => {
                let id_type = if is_aggregation {
                    "aggregation"
                } else {
                    "batch"
                };
                if let Ok(id) = Self::read_risc0_id(id_type) {
                    if is_aggregation {
                        image_id.risc0_agg_id = Some(id);
                    } else {
                        image_id.risc0_batch_id = Some(id);
                    }
                }
            }
            ProofType::Sp1 => {
                let hash_type = if is_aggregation {
                    "aggregation"
                } else {
                    "batch"
                };
                if let Ok(hash) = Self::read_sp1_vk_hash(hash_type) {
                    if is_aggregation {
                        image_id.sp1_agg_vk_hash = Some(hash);
                    } else {
                        image_id.sp1_batch_vk_hash = Some(hash);
                    }
                }
            }
            _ => {
                // For other proof types (Native, Sgx, SgxGeth), we don't need image IDs
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
