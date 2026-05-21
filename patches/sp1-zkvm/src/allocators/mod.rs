//! Allocators for the SP1 zkVM.
//!
//! The `embedded` allocator takes precedence if enabled.

#[cfg(feature = "bump")]
mod bump;

#[cfg(not(feature = "bump"))]
pub mod embedded;

#[cfg(not(feature = "bump"))]
pub use embedded::init;
