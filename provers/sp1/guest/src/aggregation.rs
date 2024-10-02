//! Aggregates multiple block proofs
#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};

use raiko_lib::{
    input::ZkAggregationGuestInput,
    primitives::B256,
    protocol_instance::{aggregation_output, words_to_bytes_be},
};

pub fn main() {
    // Read the aggregation input
    let input = sp1_zkvm::io::read::<ZkAggregationGuestInput>();

    // Verify the block proofs.
    for block_input in input.block_inputs.iter() {
        sp1_zkvm::lib::verify::verify_sp1_proof(
            &input.image_id,
            &Sha256::digest(block_input).into(),
        );
    }

    // The aggregation output
    sp1_zkvm::io::commit_slice(&aggregation_output(
        B256::from(words_to_bytes_be(&input.image_id)),
        input.block_inputs,
    ));
}

