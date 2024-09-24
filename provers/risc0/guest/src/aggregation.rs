//! Aggregates multiple block proofs
#![no_main]
harness::entrypoint!(main);

use risc0_zkvm::{guest::env, serde};

use raiko_lib::{
    input::ZkAggregationGuestInput,
    primitives::B256,
    protocol_instance::{aggregation_output, words_to_bytes_le},
};

pub fn main() {
    // Read the aggregation input
    let input = env::read::<ZkAggregationGuestInput>();

    // Verify the proofs.
    for block_input in input.block_inputs.iter() {
        env::verify(input.image_id, &serde::to_vec(block_input).unwrap()).unwrap();
    }

    // The aggregation output
    env::commit_slice(&aggregation_output(
        B256::from(words_to_bytes_le(&input.image_id)),
        input.block_inputs,
    ));
}
