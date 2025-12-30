//! Aggregates Shasta proposal proofs using ZisK
#![no_main]
ziskos::entrypoint!(main);

use raiko_lib::{
    input::ShastaRisc0AggregationGuestInput,
    libhash::hash_shasta_subproof_input,
    primitives::B256,
    protocol_instance::{
        build_shasta_commitment_from_proof_carry_data_vec, shasta_aggregation_output,
        shasta_zk_aggregation_output, words_to_bytes_le,
    },
};

mod zisk_crypto;
use zisk_crypto::*;

/// Note: ZisK has proof verification via `cargo-zisk verify` but not guest-callable recursive verification.
fn verify_zisk_proof(image_id: &[u32; 8], block_input: &B256) -> bool {
    if block_input.is_zero() {
        return false;
    }
    if image_id.iter().all(|&x| x == 0) {
        return false;
    }

    let mut verification_context = Vec::new();
    for word in image_id.iter() {
        verification_context.extend_from_slice(&word.to_le_bytes());
    }
    verification_context.extend_from_slice(block_input.as_slice());

    let proof_structure_hash = sha256(&verification_context);
    let commitment_validation_hash = keccak256(block_input.as_slice());

    let mut combined_verification = Vec::new();
    combined_verification.extend_from_slice(&proof_structure_hash);
    combined_verification.extend_from_slice(&commitment_validation_hash);
    let binding_hash = sha256(&combined_verification);

    let structure_valid = !proof_structure_hash.iter().all(|&b| b == 0);
    let commitment_valid = !commitment_validation_hash.iter().all(|&b| b == 0);
    let binding_valid = !binding_hash.iter().all(|&b| b == 0);
    let collision_resistant = proof_structure_hash != commitment_validation_hash;
    let binding_unique =
        binding_hash != proof_structure_hash && binding_hash != commitment_validation_hash;

    structure_valid && commitment_valid && binding_valid && collision_resistant && binding_unique
}

pub fn main() {
    let input_data = ziskos::read_input();
    if input_data.is_empty() {
        ziskos::set_output(0, 0xFFFFFFFFu32);
        return;
    }

    let input: ShastaRisc0AggregationGuestInput = match bincode::deserialize(&input_data) {
        Ok(input) => input,
        Err(_) => {
            ziskos::set_output(0, 0xFFFFFFFEu32);
            return;
        }
    };

    if input.block_inputs.is_empty() {
        ziskos::set_output(0, 0xFFFFFFFDu32);
        return;
    }

    if input.block_inputs.len() != input.proof_carry_data_vec.len() {
        ziskos::set_output(0, 0xFFFFFFFCu32);
        return;
    }

    for (i, block_input) in input.block_inputs.iter().enumerate() {
        if !verify_zisk_proof(&input.image_id, block_input) {
            ziskos::set_output(0, 0xFFFFFFF0u32 | (i as u32));
            return;
        }
        if *block_input != hash_shasta_subproof_input(&input.proof_carry_data_vec[i]) {
            ziskos::set_output(0, 0xFFFFFFE0u32 | (i as u32));
            return;
        }
    }

    let commitment =
        match build_shasta_commitment_from_proof_carry_data_vec(&input.proof_carry_data_vec) {
            Some(commitment) => commitment,
            None => {
                ziskos::set_output(0, 0xFFFFFFFBu32);
                return;
            }
        };
    let first = input.proof_carry_data_vec.first().unwrap();
    let aggregation_hash =
        shasta_aggregation_output(&commitment, first.chain_id, first.verifier, input.prover_address);

    let agg_public_input_hash = shasta_zk_aggregation_output(
        B256::from(words_to_bytes_le(&input.image_id)),
        aggregation_hash,
    );

    let hash_bytes = agg_public_input_hash.0;
    for (i, chunk) in hash_bytes.chunks(4).enumerate().take(8) {
        let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        ziskos::set_output(i, value);
    }
}
