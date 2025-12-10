//! Boundless-style Shasta aggregation: verifies receipts provided by the host and commits the aggregation output.
#![no_main]
harness::entrypoint!(main);

use bincode;
use revm_primitives::Address;
use risc0_zkvm::{guest::env, sha::Digest, Receipt};

use raiko_lib::{
    primitives::B256,
    protocol_instance::{shasta_aggregation_output, shasta_zk_aggregation_output, words_to_bytes_le},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundlessShastaAggregationGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<Receipt>,
    pub chain_id: u64,
    pub verifier_address: Address,
    pub prover_address: Address,
}

pub fn main() {
    // The boundless host writes a framed bincode payload; read and deserialize it.
    let input_buf: Vec<u8> = env::read_frame();
    let input: BoundlessShastaAggregationGuestInput =
        bincode::deserialize(&input_buf).expect("failed to deserialize shasta aggregation input");

    // Verify receipts and derive block inputs from their journals (matching foundational aggregation).
    let mut block_inputs: Vec<B256> = Vec::with_capacity(input.receipts.len());
    for receipt in input.receipts.iter() {
        receipt.verify(input.image_id).expect("receipt verification failed");
        // Journals are framed: 4-byte length prefix followed by the B256 bytes.
        let journal_bytes = &receipt.journal.bytes;
        let block_input = B256::from_slice(&journal_bytes[4..]);
        block_inputs.push(block_input);
    }

    let aggregation_hash = shasta_aggregation_output(
        &block_inputs,
        input.chain_id,
        input.verifier_address,
        input.prover_address,
    );

    let image_words: [u32; 8] = input
        .image_id
        .as_words()
        .try_into()
        .expect("image_id should have 8 words");
    let agg_public_input_hash = shasta_zk_aggregation_output(
        B256::from(words_to_bytes_le(&image_words)),
        aggregation_hash,
    );

    env::commit_slice(agg_public_input_hash.as_slice());
}
