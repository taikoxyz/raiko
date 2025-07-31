#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
use raiko_lib::{
    builder::calculate_batch_blocks_final_header, input::GuestBatchInput, proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};
use revm_precompile::zk_op::ZkOperation;
use risc0_zkvm::guest::env;
use zk_op::Risc0Operator;

pub mod mem;

pub use mem::*;

fn main() {
    let batch_input: GuestBatchInput = env::read();

    revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Risc0Operator {}));
    revm_precompile::zk_op::ZKVM_OPERATIONS
        .set(Box::new(vec![ZkOperation::Sha256, ZkOperation::Secp256k1]))
        .expect("Failed to set ZkvmOperations");

    let final_blocks = calculate_batch_blocks_final_header(&batch_input);
    let pi = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Risc0)
        .unwrap()
        .instance_hash();

    env::commit(&pi);
}

harness::zk_suits!(
    pub mod tests {
        #[test]
        pub fn test_build_from_mock_input() {
            use raiko_lib::input::GuestBatchInput;
            
            let batch_input = GuestBatchInput::mock();
            let final_blocks = raiko_lib::builder::calculate_batch_blocks_final_header(&batch_input);
            let pi = raiko_lib::protocol_instance::ProtocolInstance::new_batch(&batch_input, final_blocks, raiko_lib::proof_type::ProofType::Risc0)
                .unwrap()
                .instance_hash();
            
            // Verify that we can successfully process mock input
            assert!(!pi.is_zero());
        }
    }
);
