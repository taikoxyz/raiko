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
    input::ZkAggregationGuestInput,
    primitives::B256,
    protocol_instance::{aggregation_output, words_to_bytes_le},
};

risc0_zkvm::guest::entry!(main);

pub fn main() {
    let bytes = env::read_frame();
    let (image_id, receipts): (Digest, Vec<Receipt>) =
        postcard::from_bytes(&bytes).expect("failed to deserialize input");

    let mut inputs = Vec::new();
    // Verify the proofs.
    for receipt in receipts.iter() {
        let claim = receipt.claim().unwrap();
        receipt.verify(image_id).unwrap();
        inputs.push(
            B256::try_from(receipt.journal.bytes.as_ref())
                .expect("failed to convert journal bytes to B256"),
        );
    }

    // The aggregation output
    env::commit_slice(&aggregation_output(
        B256::from(words_to_bytes_le(&image_id)),
        &inputs,
    ));
}
