use axum::response::IntoResponse;
use raiko_lib::prover::ProverError;
use utoipa::ToSchema;

use crate::interfaces::request::ProofType;

/// The standardized error returned by the Raiko host.
#[derive(thiserror::Error, Debug, ToSchema)]
pub enum HostError {
    /// For invalid proof type generation request.
    #[error("Unknown proof type: {0}")]
    InvalidProofType(String),

    /// For invalid proof request configuration.
    #[error("Invalid proof request: {0}")]
    InvalidRequestConfig(String),

    /// For invalid address.
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// For I/O errors.
    #[error("There was a I/O error: {0}")]
    #[schema(value_type = Value)]
    Io(#[from] std::io::Error),

    /// For preflight errors.
    #[error("There was an error running the preflight: {0}")]
    Preflight(String),

    /// For invalid type conversion.
    #[error("Invalid conversion: {0}")]
    Conversion(String),

    /// For RPC errors.
    #[error("There was an error with the RPC provider: {0}")]
    RPC(String),

    /// For Serde errors.
    #[error("There was a deserialization error: {0}")]
    #[schema(value_type = Value)]
    Serde(#[from] serde_json::Error),

    /// For errors related to the tokio runtime.
    #[error("There was a tokio task error: {0}")]
    #[schema(value_type = Value)]
    JoinHandle(#[from] tokio::task::JoinError),

    /// For errors produced by the guest provers.
    #[error("There was an error with a guest prover: {0}")]
    #[schema(value_type = Value)]
    Guest(#[from] ProverError),

    /// For db errors.
    #[error("There was an error with the db: {0}")]
    #[schema(value_type = Value)]
    Db(raiko_lib::mem_db::DbError),

    /// For requesting a proof of a type that is not supported.
    #[error("Feature not supported: {0}")]
    #[schema(value_type = Value)]
    FeatureNotSupportedError(ProofType),

    /// A catch-all error for any other error type.
    #[error("There was an unexpected error: {0}")]
    #[schema(value_type = Value)]
    Anyhow(#[from] anyhow::Error),
}

impl From<raiko_lib::mem_db::DbError> for HostError {
    fn from(e: raiko_lib::mem_db::DbError) -> Self {
        HostError::Db(e)
    }
}

impl IntoResponse for HostError {
    fn into_response(self) -> axum::response::Response {
        let (error, message) = match self {
            HostError::InvalidProofType(e) => ("invalid_proof_type".to_string(), e),
            HostError::InvalidRequestConfig(e) => ("invalid_request_config".to_string(), e),
            HostError::InvalidAddress(e) => ("invalid_address".to_string(), e),
            HostError::Io(e) => ("io_error".to_string(), e.to_string()),
            HostError::Preflight(e) => ("preflight_error".to_string(), e),
            HostError::Conversion(e) => ("conversion_error".to_string(), e),
            HostError::RPC(e) => ("rpc_error".to_string(), e),
            HostError::Serde(e) => ("serde_error".to_string(), e.to_string()),
            HostError::JoinHandle(e) => ("join_handle_error".to_string(), e.to_string()),
            HostError::Guest(e) => ("guest_error".to_string(), e.to_string()),
            HostError::Db(e) => ("db_error".to_string(), e.to_string()),
            HostError::FeatureNotSupportedError(t) => {
                ("feature_not_supported_error".to_string(), t.to_string())
            }
            HostError::Anyhow(e) => ("anyhow_error".to_string(), e.to_string()),
        };
        axum::Json(serde_json::json!({ "status": "error", "error": error, "message": message }))
            .into_response()
    }
}

/// A type alias for the standardized result type returned by the Raiko host.
pub type HostResult<T> = axum::response::Result<T, HostError>;
