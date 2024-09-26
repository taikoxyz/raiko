#![no_main]
harness::entrypoint!(main);
use risc0_zkvm::{serde, guest::env};
use raiko_lib::protocol_instance::words_to_bytes_le;
use raiko_lib::protocol_instance::aggregation_output;
use raiko_lib::input::ZkAggregationGuestInput;
use raiko_lib::primitives::B256;

fn main() {
    // Read the aggregation input
    let input: ZkAggregationGuestInput = env::read();

    // Verify the proofs.
    for block_input in input.block_inputs.iter() {
        // Verify that n has a known factorization.
        env::verify(input.image_id, &serde::to_vec(&block_input).unwrap()).unwrap();
    }

    // The aggregation output
    env::commit(&aggregation_output(B256::from(words_to_bytes_le(&input.image_id)), input.block_inputs));
}
