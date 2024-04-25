use std::fmt;

use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum HostError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),

    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Join handle error: {0}")]
    JoinHandle(#[from] tokio::task::JoinError),

    #[error("Guest error: {0}")]
    GuestError(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Io(e) => write!(f, "{}", e),
            HostError::Anyhow(e) => write!(f, "{}", e),
            HostError::Serde(e) => write!(f, "{}", e),
            HostError::JoinHandle(e) => write!(f, "{}", e),
            HostError::GuestError(e) => write!(f, "{}", e),
        }
    }
}

pub type Result<T, E = HostError> = core::result::Result<T, E>;
