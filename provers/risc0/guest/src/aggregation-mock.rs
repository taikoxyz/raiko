#![no_main]
harness::entrypoint!(main);
use risc0_zkvm::guest::env;

pub mod mem;

pub use mem::*;

fn main() {
    env::commit_slice(&vec![0u8]);
}
