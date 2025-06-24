#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
use raiko_lib::{
    builder::calculate_block_header, input::GuestInput, proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};
use revm_precompile::zk_op::ZkOperation;
use risc0_zkvm::guest::env;
use zk_op::Risc0Operator;

pub mod mem;

pub use mem::*;

// deprecated after pacaya
fn main() {
    let input: GuestInput = env::read();

    revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Risc0Operator {}));
    revm_precompile::zk_op::ZKVM_OPERATIONS
        .set(Box::new(vec![ZkOperation::Sha256, ZkOperation::Secp256k1]))
        .expect("Failed to set ZkvmOperations");

    let header = calculate_block_header(&input);
    let pi = ProtocolInstance::new(&input, &header, ProofType::Risc0)
        .unwrap()
        .instance_hash();

    env::commit(&pi);
}

harness::zk_suits!(
    pub mod tests {
        #[test]
        pub fn test_build_from_mock_input() {
            // Todo: impl mock input for static unit test
            assert_eq!(1, 1);
        }
    }
);