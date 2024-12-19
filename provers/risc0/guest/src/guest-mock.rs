#![no_main]
harness::entrypoint!(main);
use raiko_lib::primitives::B256;
use risc0_zkvm::guest::env;

pub mod mem;

pub use mem::*;

fn main() {
    env::commit(&B256::ZERO);
}
