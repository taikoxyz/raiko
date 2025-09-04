// Stub types that exactly match raiko-lib interface  
use alloy_primitives::B256;
use serde::{Deserialize, Serialize};

// This must match raiko-lib's Proof exactly
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Proof {
    /// The proof either TEE or ZK.
    pub proof: Option<String>,
    /// The public input
    pub input: Option<B256>,
    /// The TEE quote.
    pub quote: Option<String>,
    /// The assumption UUID.
    pub uuid: Option<String>,
    /// The kzg proof.
    pub kzg_proof: Option<String>,
}

// Stub types for inputs/outputs - these can be simple since we serialize them
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestOutput;

#[derive(Debug, Clone, Serialize, Deserialize)]  
pub struct GuestBatchInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestBatchOutput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationGuestInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationGuestOutput;

// This must match raiko-lib's ProofKey exactly
pub type ProofKey = (u64, u64, B256, u8);

// Traits - simplified but compatible
#[async_trait::async_trait]
pub trait IdWrite: Send {
    async fn store_id(&mut self, key: ProofKey, id: String) -> ProverResult<()>;
    async fn remove_id(&mut self, key: ProofKey) -> ProverResult<()>;
}

#[async_trait::async_trait]
pub trait IdStore: IdWrite {
    async fn read_id(&mut self, key: ProofKey) -> ProverResult<String>;
}

// Error type that matches raiko-lib's ProverError 
#[derive(Debug, thiserror::Error)]
pub enum ProverError {
    #[error("ProverError::GuestError `{0}`")]
    GuestError(String),
    #[error("ProverError::FileIo `{0}`")]
    FileIo(#[from] std::io::Error),
    #[error("ProverError::Param `{0}`")]
    Param(#[from] serde_json::Error),
    #[error("Store error `{0}`")]
    StoreError(String),
}

impl From<String> for ProverError {
    fn from(e: String) -> Self {
        ProverError::GuestError(e)
    }
}

impl From<reqwest::Error> for ProverError {
    fn from(e: reqwest::Error) -> Self {
        ProverError::GuestError(e.to_string())
    }
}

impl From<bincode::Error> for ProverError {
    fn from(e: bincode::Error) -> Self {
        ProverError::GuestError(e.to_string())
    }
}

pub type ProverResult<T, E = ProverError> = core::result::Result<T, E>;