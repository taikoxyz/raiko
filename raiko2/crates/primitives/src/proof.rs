//! Proof types for raiko2.

use alloy_primitives::{B256, ChainId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Prover error types.
#[derive(thiserror::Error, Debug)]
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

/// Result type for prover operations.
pub type ProverResult<T, E = ProverError> = core::result::Result<T, E>;

/// Prover configuration (JSON value for flexibility).
pub type ProverConfig = serde_json::Value;

/// Key for identifying a proof: (chain_id, block_number, block_hash, proof_type).
pub type ProofKey = (ChainId, u64, B256, u8);

/// The response body of a proof request.
#[derive(
    Clone, Debug, Serialize, ToSchema, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct Proof {
    /// The proof either TEE or ZK.
    pub proof: Option<String>,
    /// The public input.
    pub input: Option<B256>,
    /// The TEE quote.
    pub quote: Option<String>,
    /// The assumption UUID.
    pub uuid: Option<String>,
    /// The kzg proof.
    pub kzg_proof: Option<String>,
}

impl std::fmt::Display for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Proof {{ proof: {:?}, input: {:?}, uuid: {:?} }}",
            self.proof
                .as_ref()
                .map(|p| format!("{}...", &p[..std::cmp::min(20, p.len())])),
            self.input,
            self.uuid
        )
    }
}

/// Trait for storing proof IDs.
#[async_trait::async_trait]
pub trait IdWrite: Send {
    async fn store_id(&mut self, key: ProofKey, id: String) -> ProverResult<()>;
    async fn remove_id(&mut self, key: ProofKey) -> ProverResult<()>;
}

/// Trait for reading proof IDs.
#[async_trait::async_trait]
pub trait IdStore: IdWrite {
    async fn read_id(&mut self, key: ProofKey) -> ProverResult<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_default() {
        let proof = Proof::default();
        assert!(proof.proof.is_none());
        assert!(proof.input.is_none());
        assert!(proof.quote.is_none());
        assert!(proof.uuid.is_none());
        assert!(proof.kzg_proof.is_none());
    }

    #[test]
    fn test_proof_display() {
        let proof = Proof {
            proof: Some("0x1234567890abcdef1234567890abcdef".to_string()),
            input: Some(B256::ZERO),
            uuid: Some("test-uuid".to_string()),
            ..Default::default()
        };
        let display = format!("{}", proof);
        // Display truncates to 20 chars + "..."
        assert!(display.contains("0x1234567890abcdef12..."));
        assert!(display.contains("test-uuid"));
    }

    #[test]
    fn test_proof_serialization() {
        let proof = Proof {
            proof: Some("test-proof".to_string()),
            input: Some(B256::ZERO),
            uuid: Some("test-uuid".to_string()),
            quote: None,
            kzg_proof: None,
        };

        let json = serde_json::to_string(&proof).unwrap();
        let deserialized: Proof = serde_json::from_str(&json).unwrap();
        assert_eq!(proof, deserialized);
    }

    #[test]
    fn test_prover_error_from_string() {
        let error: ProverError = "test error".to_string().into();
        assert!(matches!(error, ProverError::GuestError(_)));
        assert!(error.to_string().contains("test error"));
    }

    #[test]
    fn test_prover_error_from_io() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let error: ProverError = io_error.into();
        assert!(matches!(error, ProverError::FileIo(_)));
    }
}
