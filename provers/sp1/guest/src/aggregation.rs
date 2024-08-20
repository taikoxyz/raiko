//! Aggregates multiple proofs

#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::Digest;
use sha2::Sha256;

pub fn main() {
    // Read the verification key.
    let vkey = sp1_zkvm::io::read::<[u32; 8]>();
    // Read the inputs for each block proof.
    let public_inputs = sp1_zkvm::io::read::<Vec<B256>>();

    // Verify the proofs.
    for public_input in public_inputs {
        sp1_zkvm::lib::verify::verify_sp1_proof(vkey, &Sha256::digest(public_input).into());
    }

    // The aggregation output
    sp1_zkvm::io::commit_slice(&aggregation_output(&words_to_bytes_le(vkey), public_inputs));
}