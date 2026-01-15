pub mod boundless_aggregation;
pub mod boundless_batch;
pub mod boundless_shasta_aggregation;
pub mod risc0_aggregation;
pub mod risc0_batch;
pub mod risc0_shasta_aggregation;

// To build the following `$ cargo run --features test,bench --bin risc0-builder`
// or `$ $TARGET=risc0 make test`

#[cfg(feature = "bench")]
pub mod ecdsa;
#[cfg(feature = "bench")]
pub mod sha256;
#[cfg(test)]
pub mod test_risc0_batch;
