//! Boundless-style Shasta aggregation: re-verifies receipts and commits aggregation output.
#![no_main]
harness::entrypoint!(main);

use risc0_zkvm::{guest::env, serde};

use raiko_lib::{
    input::ShastaRisc0AggregationGuestInput,
    primitives::B256,
    protocol_instance::{shasta_aggregation_output,shasta_zk_aggregation_output, words_to_bytes_le}
};

pub fn main() {
    let input = env::read::<ShastaRisc0AggregationGuestInput>();

    for block_input in input.block_inputs.iter() {
        // In boundless flow, receipts are verified externally; here we re-verify inputs against the image id.
        env::verify(input.image_id, &serde::to_vec(block_input).unwrap()).unwrap();
    }

    let aggregation_hash = shasta_aggregation_output(
        &input.block_inputs,
        input.chain_id,
        input.verifier_address,
        input.prover_address,
    );

    let agg_public_input_hash = shasta_zk_aggregation_output(
        B256::from(words_to_bytes_le(&input.image_id)),
        aggregation_hash,
    );

    env::commit_slice(agg_public_input_hash.as_slice());
}
