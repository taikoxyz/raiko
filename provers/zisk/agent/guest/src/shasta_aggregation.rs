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

pub fn main() {
    let input_data = ziskos::read_input();
    assert!(!input_data.is_empty(), "shasta aggregation input is empty");

    let input: ShastaRisc0AggregationGuestInput = bincode::deserialize(&input_data)
        .expect("failed to deserialize ShastaRisc0AggregationGuestInput");

    assert!(
        !input.block_inputs.is_empty(),
        "shasta aggregation input has no block inputs"
    );

    assert_eq!(
        input.block_inputs.len(),
        input.proof_carry_data_vec.len(),
        "block inputs and proof carry data length mismatch"
    );

    for (i, block_input) in input.block_inputs.iter().enumerate() {
        let expected = hash_shasta_subproof_input(&input.proof_carry_data_vec[i]);
        assert_eq!(
            *block_input, expected,
            "shasta block input {} does not match expected hash",
            i
        );
    }

    let commitment =
        build_shasta_commitment_from_proof_carry_data_vec(&input.proof_carry_data_vec)
            .expect("failed to build shasta commitment");
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
