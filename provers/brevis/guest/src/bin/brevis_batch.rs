#![no_main]

use pico_sdk::io::{commit_bytes, read_as};
use raiko_lib::{
    builder::calculate_batch_blocks_final_header, input::GuestBatchInput,
    proof_type::ProofType, protocol_instance::ProtocolInstance,
};

pico_sdk::entrypoint!(main);

pub fn main() {
    let batch_input: GuestBatchInput = read_as();
    let final_blocks = calculate_batch_blocks_final_header(&batch_input);
    let instance_hash = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::BrevisPico)
        .expect("failed to build protocol instance")
        .instance_hash();

    commit_bytes(&instance_hash.0);
}
