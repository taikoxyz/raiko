//! Aggregates Shasta proposal proofs for Brevis Pico
#![no_main]

#[path = "../mem.rs"]
pub mod mem;

use pico_sdk::{
    io::{commit_bytes, read_as},
    verify::verify_pico_proof,
};
use raiko_lib::{
    input::ShastaBrevisAggregationGuestInput,
    libhash::hash_shasta_subproof_input,
    primitives::B256,
    protocol_instance::{shasta_aggregation_hash_for_zk, words_to_bytes_le},
};
use sha2::{Digest, Sha256};

pico_sdk::entrypoint!(main);

pub fn main() {
    let input: ShastaBrevisAggregationGuestInput = read_as();

    assert_eq!(input.block_inputs.len(), input.proof_carry_data_vec.len());

    for (i, block_input) in input.block_inputs.iter().enumerate() {
        let pv_digest: [u8; 32] = Sha256::digest(block_input.as_slice()).into();
        verify_pico_proof(&input.image_id, &pv_digest);
        assert_eq!(
            *block_input,
            hash_shasta_subproof_input(&input.proof_carry_data_vec[i])
        );
    }

    let sub_image_id = B256::from(words_to_bytes_le(&input.image_id));
    let agg_public_input_hash = shasta_aggregation_hash_for_zk(
        sub_image_id,
        &input.proof_carry_data_vec,
    )
    .expect("invalid shasta proof carry data");

    commit_bytes(agg_public_input_hash.as_slice());
}
