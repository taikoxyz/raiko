#![no_main]
harness::entrypoint!(main);
use raiko_lib::{
    builder::calculate_batch_blocks_final_header, input::GuestBatchInput, proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};
use risc0_zkvm::guest::env;

fn main() {
    let mut batch_input: GuestBatchInput = env::read();

    let final_blocks = calculate_batch_blocks_final_header(&mut batch_input);
    let pi = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Risc0)
        .unwrap()
        .instance_hash();

    env::commit(&pi);
}
