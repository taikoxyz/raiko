//! Aggregates Shasta proposal proofs on SP1
#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};

use raiko_lib::{
    input::ShastaSp1AggregationGuestInput,
    libhash::hash_shasta_subproof_input,
    primitives::B256,
    protocol_instance::{
        shasta_aggregation_hash_for_zk, words_to_bytes_be,
    },
};

pub fn main() {
    // Read aggregation input prepared by the host
    let input = sp1_zkvm::io::read::<ShastaSp1AggregationGuestInput>();

    assert_eq!(input.block_inputs.len(), input.proof_carry_data_vec.len());

    // Re-verify every underlying SP1 proof against the provided image id.
    for (i, block_input) in input.block_inputs.iter().enumerate() {
        sp1_zkvm::lib::verify::verify_sp1_proof(
            &input.image_id,
            &Sha256::digest(block_input.as_slice()).into(),
        );
        assert_eq!(*block_input, hash_shasta_subproof_input(&input.proof_carry_data_vec[i]));
    }

    // Compute the aggregation hash expected by the Shasta verifier.
    let sub_image_id = B256::from(words_to_bytes_be(&input.image_id));
    let agg_public_input_hash =
        shasta_aggregation_hash_for_zk(sub_image_id, &input.proof_carry_data_vec)
            .expect("invalid shasta proof carry data");

    sp1_zkvm::io::commit_slice(agg_public_input_hash.as_slice());
}
