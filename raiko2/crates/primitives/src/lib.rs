//! Raiko2 primitives - core types for the Raiko V2 prover.
//!
//! This crate provides the foundational types used throughout the Raiko V2 system,
//! including input/output types for guest programs, proof types, and error handling.

mod context;
mod error;
mod input;
pub mod instance;
mod output;
mod proof;

pub use context::ProofContext;
pub use error::{RaikoError, RaikoResult, RaizenError, RaizenResult};
pub use input::{
    AggregationGuestInput, BlobProofType, GuestInput, RawAggregationGuestInput, RawProof,
    StatelessInput, TaikoManifest, TaikoProverData, ZkAggregationGuestInput,
};
pub use output::{AggregationGuestOutput, GuestBatchOutput, GuestOutput};
pub use proof::{IdStore, IdWrite, Proof, ProofKey, ProverConfig, ProverError, ProverResult};
