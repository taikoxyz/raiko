// Copyright (c) 2025 RISC Zero, Inc.
//
// All rights reserved.

//! Verify the receipt given as input and commit to its claim digest.

#![no_main]

use risc0_zkvm::{
    guest::env,
    sha::{Digest, Digestible},
    Receipt,
};

use raiko_lib::{
    primitives::B256,
    protocol_instance::{aggregation_output, words_to_bytes_le},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundlessAggregationGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<Receipt>,
}

risc0_zkvm::guest::entry!(main);

pub fn main() {
    let input_buf: Vec<u8> = env::read_frame();
    let input: BoundlessAggregationGuestInput =
        bincode::deserialize::<BoundlessAggregationGuestInput>(&input_buf).unwrap();

    let image_id = input.image_id;
    let mut public_inputs = Vec::new();
    // Verify the proofs.
    for receipt in input.receipts.iter() {
        receipt.verify(image_id).unwrap();
        public_inputs.push(B256::from_slice(&receipt.journal.bytes[4..]));
    }

    // The aggregation output
    env::commit_slice(&aggregation_output(
        B256::from(words_to_bytes_le(image_id.as_ref())),
        public_inputs,
    ));
}
