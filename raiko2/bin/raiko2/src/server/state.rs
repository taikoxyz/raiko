//! Application state for the HTTP server.

use crate::config::Config;
use anyhow::Result;
use std::sync::Arc;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    // TODO: Add engine, prover, etc.
}

impl AppState {
    /// Create new application state.
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config: Arc::new(config),
        })
    }
}
