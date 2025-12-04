//! Aggregates Shasta proposal proofs on RISC0
#![no_main]
harness::entrypoint!(main);

use risc0_zkvm::{guest::env, serde};

use raiko_lib::{
    input::ShastaRisc0AggregationGuestInput,
    protocol_instance::{shasta_aggregation_output, shasta_zk_aggregation_output},
};

pub fn main() {
    let input = env::read::<ShastaRisc0AggregationGuestInput>();

    for block_input in input.block_inputs.iter() {
        env::verify(input.image_id, &serde::to_vec(block_input).unwrap()).unwrap();
    }

    let aggregation_hash = shasta_aggregation_output(
        &input.block_inputs,
        input.chain_id,
        input.verifier_address,
        input.prover_address,
    );

    let agg_public_input_hash = shasta_zk_aggregation_output(
        B256::from(words_to_bytes_be(&input.image_id)),
        aggregation_hash,
    );

    env::commit_slice(aggregation_hash.as_slice());
}
