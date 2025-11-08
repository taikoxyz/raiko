//! Aggregates Shasta proposal proofs on SP1
#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};

use raiko_lib::{
    input::ShastaSp1AggregationGuestInput,
    protocol_instance::shasta_aggregation_output,
};

pub fn main() {
    // Read aggregation input prepared by the host
    let input = sp1_zkvm::io::read::<ShastaSp1AggregationGuestInput>();

    // Re-verify every underlying SP1 proof against the provided image id.
    for block_input in input.block_inputs.iter() {
        sp1_zkvm::lib::verify::verify_sp1_proof(
            &input.image_id,
            &Sha256::digest(block_input).into(),
        );
    }

    // Compute the aggregation hash expected by the Shasta verifier.
    let aggregation_hash = shasta_aggregation_output(
        &input.block_inputs,
        input.chain_id,
        input.verifier_address,
        input.prover_address,
    );

    sp1_zkvm::io::commit_slice(aggregation_hash.as_slice());
}
