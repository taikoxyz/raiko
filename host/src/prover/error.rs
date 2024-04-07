use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum HostError {
    Io(std::io::Error),
    Anyhow(#[from] anyhow::Error),
    Serde(serde_json::Error),
    JoinHandle(tokio::task::JoinError),
    GuestError(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Io(e) => e.fmt(f),
            HostError::Anyhow(e) => e.fmt(f),
            HostError::Serde(e) => e.fmt(f),
            HostError::JoinHandle(e) => e.fmt(f),
            HostError::GuestError(e) => e.fmt(f),
        }
    }
}

impl From<std::io::Error> for HostError {
    fn from(e: std::io::Error) -> Self {
        HostError::Io(e)
    }
}

impl From<serde_json::Error> for HostError {
    fn from(e: serde_json::Error) -> Self {
        HostError::Serde(e)
    }
}

impl From<tokio::task::JoinError> for HostError {
    fn from(e: tokio::task::JoinError) -> Self {
        HostError::JoinHandle(e)
    }
}

impl From<String> for HostError {
    fn from(e: String) -> Self {
        HostError::GuestError(e)
    }
}

pub type Result<T, E = HostError> = core::result::Result<T, E>;
