//! Shasta protocol error types.

use thiserror::Error;

/// Protocol-specific errors.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Insufficient bytes in payload.
    #[error("insufficient bytes: expected {expected} at offset {offset}, got {actual}")]
    InsufficientBytes {
        expected: usize,
        offset: usize,
        actual: usize,
    },

    /// Invalid version in payload.
    #[error("invalid version: expected {expected}, got {actual}")]
    InvalidVersion { expected: u8, actual: u8 },

    /// Invalid bond type.
    #[error("invalid bond type: {0}")]
    InvalidBondType(u8),

    /// RLP decoding error.
    #[error("RLP decode error: {0}")]
    RlpDecode(String),

    /// Compression/decompression error.
    #[error("compression error: {0}")]
    Compression(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic error.
    #[error("{0}")]
    Other(String),
}

/// Result type for protocol operations.
pub type Result<T> = std::result::Result<T, ProtocolError>;

impl From<alloy_rlp::Error> for ProtocolError {
    fn from(e: alloy_rlp::Error) -> Self {
        ProtocolError::RlpDecode(e.to_string())
    }
}
