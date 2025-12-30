// Re-export types from raiko_lib to ensure consistency
pub use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput, ZkAggregationGuestInput,
    },
    prover::Proof,
};
use alloy_primitives::B256;
use raiko_lib::prover::ProverError as RaikoProverError;

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

impl From<ProverError> for RaikoProverError {
    fn from(e: ProverError) -> Self {
        match e {
            ProverError::GuestError(msg) => RaikoProverError::GuestError(msg),
            ProverError::FileIo(err) => RaikoProverError::FileIo(err),
            ProverError::Param(err) => RaikoProverError::Param(err),
            ProverError::StoreError(msg) => RaikoProverError::StoreError(msg),
        }
    }
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
