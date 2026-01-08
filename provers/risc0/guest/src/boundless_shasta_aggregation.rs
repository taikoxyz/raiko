//! Boundless-style Shasta aggregation: verifies receipts provided by the host and commits the aggregation output.
#![no_main]
harness::entrypoint!(main);

use bincode;
use risc0_zkvm::{guest::env, sha::Digest, Receipt};

use raiko_lib::{
    libhash::hash_shasta_subproof_input,
    primitives::B256,
    protocol_instance::{
        shasta_aggregation_hash_for_zk, words_to_bytes_le,
    },
    prover::ProofCarryData,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundlessShastaAggregationGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<Receipt>,
    pub proof_carry_data_vec: Vec<ProofCarryData>,
}

pub fn main() {
    // The boundless host writes a framed bincode payload; read and deserialize it.
    let input_buf: Vec<u8> = env::read_frame();
    let input: BoundlessShastaAggregationGuestInput =
        bincode::deserialize(&input_buf).expect("failed to deserialize shasta aggregation input");

    assert_eq!(
        input.receipts.len(),
        input.proof_carry_data_vec.len(),
        "receipts and proof_carry_data_vec must be the same length"
    );

    // Verify receipts and ensure each receipt journal matches the expected shasta subproof input.
    for (i, receipt) in input.receipts.iter().enumerate() {
        receipt.verify(input.image_id).expect("receipt verification failed");

        // Journals are framed: 4-byte length prefix followed by the bytes (for a B256, 32 bytes).
        let journal_bytes = &receipt.journal.bytes;
        assert!(
            journal_bytes.len() >= 4,
            "receipt journal too short for length prefix"
        );
        let len = u32::from_le_bytes(journal_bytes[0..4].try_into().unwrap()) as usize;
        assert_eq!(len, 32, "expected journal length prefix to be 32");
        assert_eq!(
            journal_bytes.len(),
            4 + len,
            "receipt journal length mismatch"
        );

        let block_input = B256::from_slice(&journal_bytes[4..]);
        let expected = hash_shasta_subproof_input(&input.proof_carry_data_vec[i]);
        assert_eq!(
            block_input, expected,
            "receipt journal does not match hash_shasta_subproof_input"
        );
    }

    let image_words: [u32; 8] = input
        .image_id
        .as_words()
        .try_into()
        .expect("image_id should have 8 words");
    let sub_image_id = B256::from(words_to_bytes_le(&image_words));
    let agg_public_input_hash =
        shasta_aggregation_hash_for_zk(sub_image_id, &input.proof_carry_data_vec)
            .expect("invalid shasta proof carry data");

    env::commit_slice(agg_public_input_hash.as_slice());
}
