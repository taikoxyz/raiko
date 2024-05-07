use axum::{http::StatusCode, response::IntoResponse};
use raiko_lib::prover::ProverError;
use utoipa::ToSchema;

use crate::request::ProofType;

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
    #[error("There was a error running the preflight: {0}")]
    Preflight(String),

    /// For invalid type conversion.
    #[error("Invalid conversion: {0}")]
    Conversion(String),

    /// For RPC errors.
    #[error("There was a error with the RPC provider: {0}")]
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
    #[error("There was a error with a guest prover: {0}")]
    #[schema(value_type = Value)]
    Guest(#[from] ProverError),

    /// For db errors.
    #[error("There was a error with the db: {0}")]
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
        use HostError::*;
        match self {
            InvalidProofType(e) | InvalidRequestConfig(e) | InvalidAddress(e) => {
                (StatusCode::BAD_REQUEST, e.to_string()).into_response()
            }
            Conversion(e) | Preflight(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
            Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            Serde(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            Anyhow(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            JoinHandle(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            Guest(e) => (StatusCode::FAILED_DEPENDENCY, e.to_string()).into_response(),
            RPC(e) => (StatusCode::FAILED_DEPENDENCY, e.to_string()).into_response(),
            Db(e) => (StatusCode::FAILED_DEPENDENCY, e.to_string()).into_response(),
            FeatureNotSupportedError(e) => {
                (StatusCode::METHOD_NOT_ALLOWED, e.to_string()).into_response()
            }
        }
    }
}

/// A type alias for the standardized result type returned by the Raiko host.
pub type HostResult<T> = axum::response::Result<T, HostError>;
