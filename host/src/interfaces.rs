use axum::response::IntoResponse;
use raiko_lib::proof_type::ProofType;
use raiko_lib::prover::ProverError;
use raiko_tasks::TaskStatus;
use tokio::sync::mpsc::error::TrySendError;
use utoipa::ToSchema;

use crate::server::api::v1::Status;

/// The standardized error returned by the Raiko host.
#[derive(thiserror::Error, Debug, ToSchema)]
pub enum HostError {
    /// For unexpectedly dropping task handle.
    #[error("Task handle unexpectedly dropped")]
    HandleDropped,

    /// For full prover capacity.
    #[error("Capacity full")]
    CapacityFull,

    /// For invalid address.
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// For invalid proof request configuration.
    #[error("Invalid proof request: {0}")]
    InvalidRequestConfig(String),

    /// For I/O errors.
    #[error("There was a I/O error: {0}")]
    #[schema(value_type = Value)]
    Io(#[from] std::io::Error),

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

    /// For errors from the core of Raiko.
    #[error("There was an error with the core: {0}")]
    #[schema(value_type = Value)]
    Core(#[from] raiko_core::interfaces::RaikoError),

    /// For requesting a proof of a type that is not supported.
    #[error("Feature not supported: {0}")]
    #[schema(value_type = Value)]
    FeatureNotSupportedError(ProofType),

    /// A catch-all error for any other error type.
    #[error("There was an unexpected error: {0}")]
    #[schema(value_type = Value)]
    Anyhow(#[from] anyhow::Error),

    /// For system paused state.
    #[error("System is paused")]
    SystemPaused,
}

impl IntoResponse for HostError {
    fn into_response(self) -> axum::response::Response {
        let (error, message) = match self {
            HostError::InvalidRequestConfig(e) => ("invalid_request_config", e),
            HostError::InvalidAddress(e) => ("invalid_address", e),
            HostError::Io(e) => ("io_error", e.to_string()),
            HostError::Conversion(e) => ("conversion_error", e),
            HostError::RPC(e) => ("rpc_error", e),
            HostError::Serde(e) => ("serde_error", e.to_string()),
            HostError::JoinHandle(e) => ("join_handle_error", e.to_string()),
            HostError::Guest(e) => ("guest_error", e.to_string()),
            HostError::Core(e) => ("core_error", e.to_string()),
            HostError::FeatureNotSupportedError(e) => ("feature_not_supported", e.to_string()),
            HostError::Anyhow(e) => ("anyhow_error", e.to_string()),
            HostError::HandleDropped => ("handle_dropped", "".to_owned()),
            HostError::CapacityFull => ("capacity_full", "".to_owned()),
            HostError::SystemPaused => ("system_paused", "".to_owned()),
        };
        let status = Status::Error {
            error: error.to_owned(),
            message,
        };
        let value = serde_json::to_value(status).expect("couldn't serialize the error status");
        axum::Json(value).into_response()
    }
}

impl<T> From<TrySendError<T>> for HostError {
    fn from(value: TrySendError<T>) -> Self {
        match value {
            TrySendError::Full(_) => HostError::CapacityFull,
            TrySendError::Closed(_) => HostError::HandleDropped,
        }
    }
}

impl From<String> for HostError {
    fn from(msg: String) -> Self {
        HostError::Anyhow(anyhow::anyhow!(msg))
    }
}

/// A type alias for the standardized result type returned by the Raiko host.
pub type HostResult<T> = axum::response::Result<T, HostError>;

impl From<HostError> for TaskStatus {
    fn from(value: HostError) -> Self {
        match value {
            HostError::HandleDropped
            | HostError::CapacityFull
            | HostError::JoinHandle(_)
            | HostError::InvalidAddress(_)
            | HostError::InvalidRequestConfig(_) => unreachable!(),
            HostError::Conversion(e) => TaskStatus::IoFailure(e),
            HostError::Serde(e) => TaskStatus::IoFailure(e.to_string()),
            HostError::Core(e) => TaskStatus::GuestProverFailure(e.to_string()),
            HostError::Anyhow(e) => TaskStatus::AnyhowError(e.to_string()),
            HostError::FeatureNotSupportedError(_) => TaskStatus::InvalidOrUnsupportedBlock,
            HostError::Io(e) => TaskStatus::IoFailure(e.to_string()),
            HostError::RPC(e) => TaskStatus::NetworkFailure(e.to_string()),
            HostError::Guest(e) => TaskStatus::GuestProverFailure(e.to_string()),
            HostError::SystemPaused => TaskStatus::SystemPaused,
        }
    }
}

impl From<&HostError> for TaskStatus {
    fn from(value: &HostError) -> Self {
        match value {
            HostError::HandleDropped
            | HostError::CapacityFull
            | HostError::JoinHandle(_)
            | HostError::InvalidAddress(_)
            | HostError::InvalidRequestConfig(_) => unreachable!(),
            HostError::Conversion(e) => TaskStatus::GuestProverFailure(e.to_owned()),
            HostError::Serde(e) => TaskStatus::GuestProverFailure(e.to_string()),
            HostError::Core(e) => TaskStatus::GuestProverFailure(e.to_string()),
            HostError::Anyhow(e) => TaskStatus::AnyhowError(e.to_string()),
            HostError::FeatureNotSupportedError(e) => TaskStatus::GuestProverFailure(e.to_string()),
            HostError::Io(e) => TaskStatus::GuestProverFailure(e.to_string()),
            HostError::RPC(e) => TaskStatus::NetworkFailure(e.to_string()),
            HostError::Guest(e) => TaskStatus::GuestProverFailure(e.to_string()),
            HostError::SystemPaused => TaskStatus::SystemPaused,
        }
    }
}
