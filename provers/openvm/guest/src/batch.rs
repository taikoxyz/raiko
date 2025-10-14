#![no_main]

openvm::entry!(main);

use raiko_lib::{
    builder::calculate_batch_blocks_final_header,
    input::GuestBatchInput,
    proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};
use revm_precompile::zk_op::ZkOperation;
use zk_op::OpenVMOperator;

pub mod mem;
pub use mem::*;

fn main() {
    // Read input from stdin using OpenVM's io
    let input_bytes: Vec<u8> = openvm::io::read_vec();
    let batch_input: GuestBatchInput = bincode::deserialize(&input_bytes)
        .expect("Failed to deserialize GuestBatchInput");

    // Initialize zkVM operator
    revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(OpenVMOperator {}));
    revm_precompile::zk_op::ZKVM_OPERATIONS
        .set(Box::new(vec![ZkOperation::Sha256, ZkOperation::Secp256k1]))
        .expect("Failed to set ZkvmOperations");

    // Calculate final block header
    let final_blocks = calculate_batch_blocks_final_header(&batch_input);

    // Generate protocol instance hash
    let pi = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::OpenVM)
        .expect("Failed to create protocol instance")
        .instance_hash();

    // Commit the output hash (OpenVM uses reveal_bytes32 for 32-byte outputs)
    openvm::io::reveal_bytes32(pi.0);
}
