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
    GuestError(#[from] ProverError),

    /// For requesting a proof of a type that is not supported.
    #[error("Feature not supported: {0}")]
    #[schema(value_type = Value)]
    FeatureNotSupportedError(ProofType),

    /// A catch-all error for any other error type.
    #[error("There was an unexpected error: {0}")]
    #[schema(value_type = Value)]
    Anyhow(#[from] anyhow::Error),
}

impl IntoResponse for HostError {
    fn into_response(self) -> axum::response::Response {
        match self {
            HostError::InvalidProofType(e)
            | HostError::InvalidRequestConfig(e)
            | HostError::InvalidAddress(e) => {
                (StatusCode::BAD_REQUEST, e.to_string()).into_response()
            }
            HostError::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            HostError::Serde(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
            HostError::Anyhow(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
            HostError::JoinHandle(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
            HostError::GuestError(e) => {
                (StatusCode::FAILED_DEPENDENCY, e.to_string()).into_response()
            }
            HostError::FeatureNotSupportedError(e) => {
                (StatusCode::METHOD_NOT_ALLOWED, e.to_string()).into_response()
            }
        }
    }
}

/// A type alias for the standardized result type returned by the Raiko host.
pub type HostResult<T> = axum::response::Result<T, HostError>;
