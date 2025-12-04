//! Raiko V2 Prover SDKs
//!
//! This crate provides the prover implementations for generating zero-knowledge proofs
//! of Taiko block execution. It supports multiple proving backends:
//!
//! - **RISC0**: RISC-V zkVM prover
//! - **SP1**: Succinct zkVM prover
//!
//! ## Usage
//!
//! ```rust,ignore
//! use raiko2_prover::{risc0::Risc0Prover, sp1::Sp1Prover};
//!
//! // Create RISC0 prover
//! let risc0_prover = Risc0Prover::new(Default::default());
//!
//! // Create SP1 prover
//! let sp1_prover = Sp1Prover::new(Default::default());
//! ```

#[cfg(feature = "risc0")]
pub mod risc0;

#[cfg(feature = "sp1")]
pub mod sp1;

use raiko2_primitives::{AggregationGuestInput, GuestInput, Proof, ProverConfig, ProverResult};

#[cfg(feature = "risc0")]
pub use risc0::{Risc0Config, Risc0Prover};

#[cfg(feature = "sp1")]
pub use sp1::{Sp1Config, Sp1Prover};

/// Common prover trait for all proving backends.
#[async_trait::async_trait]
pub trait Prover: Send + Sync {
    /// Generate a proof for the given input.
    async fn prove(&self, input: GuestInput, config: &ProverConfig) -> ProverResult<Proof>;

    /// Generate an aggregation proof.
    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        config: &ProverConfig,
    ) -> ProverResult<Proof>;
}
