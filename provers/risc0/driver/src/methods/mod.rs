//! RISC0 prover method definitions and configurations
//! This module handles both production and test mock implementations.

// Production implementations
#[cfg(not(feature = "test-mock-guest"))]
mod risc0_aggregation;
#[cfg(not(feature = "test-mock-guest"))]
mod risc0_guest;

// Test mock implementations
#[cfg(feature = "test-mock-guest")]
mod risc0_aggregation_mock;
#[cfg(feature = "test-mock-guest")]
mod risc0_guest_mock;

// Re-exports for production environment
#[cfg(not(feature = "test-mock-guest"))]
pub use risc0_aggregation::{RISC0_AGGREGATION_ELF, RISC0_AGGREGATION_ID};
#[cfg(not(feature = "test-mock-guest"))]
pub use risc0_guest::{RISC0_GUEST_ELF, RISC0_GUEST_ID};

// Re-exports for test environment with mock implementations
#[cfg(feature = "test-mock-guest")]
pub use risc0_aggregation_mock::{
    RISC0_AGGREGATION_MOCK_ELF as RISC0_AGGREGATION_ELF,
    RISC0_AGGREGATION_MOCK_ID as RISC0_AGGREGATION_ID,
};
#[cfg(feature = "test-mock-guest")]
pub use risc0_guest_mock::{
    RISC0_GUEST_MOCK_ELF as RISC0_GUEST_ELF, RISC0_GUEST_MOCK_ID as RISC0_GUEST_ID,
};

// To build the following `$ cargo run --features test,bench --bin risc0-builder`
// or `$ $TARGET=risc0 make test`

// Benchmark-specific modules
#[cfg(feature = "bench")]
pub mod ecdsa;
#[cfg(feature = "bench")]
pub mod sha256;

// Test-specific modules
#[cfg(test)]
pub mod test_risc0_guest;
