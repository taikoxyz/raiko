//! Error types for raiko2.

use crate::proof::ProverError;
use utoipa::ToSchema;

/// Main error type for Raiko operations.
#[derive(Debug, thiserror::Error, ToSchema)]
pub enum RaikoError {
    /// For invalid proof type generation request.
    #[error("Unknown proof type: {0}")]
    InvalidProofType(String),

    /// For invalid blob option.
    #[error("Invalid blob option: {0}")]
    InvalidBlobOption(String),

    /// For invalid proof request configuration.
    #[error("Invalid proof request: {0}")]
    InvalidRequestConfig(String),

    /// For requesting a proof of a type that is not supported.
    #[error("Feature not supported: {0}")]
    #[schema(value_type = Value)]
    FeatureNotSupportedError(String),

    /// For invalid type conversion.
    #[error("Invalid conversion: {0}")]
    Conversion(String),

    /// For RPC errors.
    #[error("There was an error with the RPC provider: {0}")]
    RPC(String),

    /// For preflight errors.
    #[error("There was an error running the preflight: {0}")]
    Preflight(String),

    /// For errors produced by the guest provers.
    #[error("There was an error with a guest prover: {0}")]
    #[schema(value_type = Value)]
    Guest(#[from] ProverError),

    /// For I/O errors.
    #[error("There was an I/O error: {0}")]
    Io(String),

    /// For serialization errors.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// For anyhow errors.
    #[error("Error: {0}")]
    Anyhow(String),

    /// For stateless validation errors.
    #[error("Stateless validation error: {0}")]
    StatelessValidation(String),
}

impl From<std::io::Error> for RaikoError {
    fn from(e: std::io::Error) -> Self {
        RaikoError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for RaikoError {
    fn from(e: serde_json::Error) -> Self {
        RaikoError::Serialization(e.to_string())
    }
}

impl From<anyhow::Error> for RaikoError {
    fn from(e: anyhow::Error) -> Self {
        RaikoError::Anyhow(e.to_string())
    }
}

impl From<reth_stateless::validation::StatelessValidationError> for RaikoError {
    fn from(e: reth_stateless::validation::StatelessValidationError) -> Self {
        RaikoError::StatelessValidation(format!("{:?}", e))
    }
}

/// Alias for backwards compatibility.
pub type RaizenError = RaikoError;

/// Result type for Raiko operations.
pub type RaikoResult<T> = Result<T, RaikoError>;

/// Alias for backwards compatibility.
pub type RaizenResult<T> = RaikoResult<T>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RaikoError::InvalidProofType("unknown".to_string());
        assert!(err.to_string().contains("Unknown proof type"));
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let err: RaikoError = io_err.into();
        assert!(matches!(err, RaikoError::Io(_)));
    }

    #[test]
    fn test_error_from_serde() {
        let json_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let err: RaikoError = json_err.into();
        assert!(matches!(err, RaikoError::Serialization(_)));
    }

    #[test]
    fn test_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("test error");
        let err: RaikoError = anyhow_err.into();
        assert!(matches!(err, RaikoError::Anyhow(_)));
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_all_error_variants() {
        // Ensure all error variants have proper Display impl
        let errors = vec![
            RaikoError::InvalidProofType("test".into()),
            RaikoError::InvalidBlobOption("test".into()),
            RaikoError::InvalidRequestConfig("test".into()),
            RaikoError::FeatureNotSupportedError("test".into()),
            RaikoError::Conversion("test".into()),
            RaikoError::RPC("test".into()),
            RaikoError::Preflight("test".into()),
            RaikoError::Io("test".into()),
            RaikoError::Serialization("test".into()),
            RaikoError::Anyhow("test".into()),
            RaikoError::StatelessValidation("test".into()),
        ];

        for err in errors {
            // Each error should have a non-empty display string
            assert!(!err.to_string().is_empty());
        }
    }
}
