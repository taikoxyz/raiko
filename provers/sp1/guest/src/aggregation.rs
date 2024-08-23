//! Aggregates multiple block proofs

#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::Digest;
use sha2::Sha256;

pub fn main() {
    // Read the aggregation input
    let input = sp1_zkvm::io::read::<ZkAggregationGuestInput>();

    // Verify the block proofs.
    for block_input in input.block_inputs {
        sp1_zkvm::lib::verify::verify_sp1_proof(vkey, &Sha256::digest(block_input).into());
    }

    // The aggregation output
    sp1_zkvm::io::commit_slice(&aggregation_output(&words_to_bytes_le(input.image_id), input.block_inputs));
}