//! Aggregates Shasta proposal proofs on RISC0
#![no_main]
harness::entrypoint!(main);

use risc0_zkvm::{guest::env, serde};

use raiko_lib::{
    input::ShastaRisc0AggregationGuestInput,
    libhash::hash_shasta_subproof_input,
    primitives::B256,
    protocol_instance::{
        build_shasta_commitment_from_proof_carry_data_vec, shasta_aggregation_output,
        shasta_zk_aggregation_output, words_to_bytes_le,
    },
};

pub fn main() {
    let input = env::read::<ShastaRisc0AggregationGuestInput>();

    assert_eq!(input.block_inputs.len(), input.proof_carry_data_vec.len());

    for (i, block_input) in input.block_inputs.iter().enumerate() {
        env::verify(input.image_id, &serde::to_vec(block_input).unwrap()).unwrap();
        assert_eq!(*block_input, hash_shasta_subproof_input(&input.proof_carry_data_vec[i]));
    }

    let commitment =
        build_shasta_commitment_from_proof_carry_data_vec(&input.proof_carry_data_vec).unwrap();
    let first = input.proof_carry_data_vec.first().unwrap();
    let aggregation_hash =
        shasta_aggregation_output(&commitment, first.chain_id, first.verifier, input.prover_address);

    let agg_public_input_hash = shasta_zk_aggregation_output(
        B256::from(words_to_bytes_le(&input.image_id)),
        aggregation_hash,
    );

    env::commit_slice(agg_public_input_hash.as_slice());
}
