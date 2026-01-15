//! Aggregates multiple block proofs for Brevis Pico
#![no_main]

#[path = "../mem.rs"]
pub mod mem;

use pico_sdk::{
    io::{commit_bytes, read_as},
    verify::verify_pico_proof,
};
use raiko_lib::{
    input::ZkAggregationGuestInput,
    primitives::B256,
    protocol_instance::{aggregation_output, words_to_bytes_le},
};
use sha2::{Digest, Sha256};

pico_sdk::entrypoint!(main);

pub fn main() {
    let input: ZkAggregationGuestInput = read_as();

    for block_input in input.block_inputs.iter() {
        let pv_digest: [u8; 32] = Sha256::digest(block_input.as_slice()).into();
        verify_pico_proof(&input.image_id, &pv_digest);
    }

    let output = aggregation_output(
        B256::from(words_to_bytes_le(&input.image_id)),
        input.block_inputs,
    );
    commit_bytes(&output);
}
