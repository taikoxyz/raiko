#![no_main]
harness::entrypoint!(main);
use raiko_lib::{
    builder::calculate_block_header, input::GuestInput, proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};
use risc0_zkvm::guest::env;

// deprecated after pacaya
fn main() {
    let mut input: GuestInput = env::read();

    let header = calculate_block_header(&mut input);
    let pi = ProtocolInstance::new(&input, &header, ProofType::Risc0)
        .unwrap()
        .instance_hash();

    env::commit(&pi);
}
