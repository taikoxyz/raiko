#![cfg(feature = "enable")]

// Local type definitions (isolated from raiko-lib to avoid dependency conflicts)
pub mod types;

// Agent HTTP client
pub mod agent;

// Re-exports for easy access
pub use agent::ZiskAgentProver;
pub use types::{Proof, ProverError, ProverResult};