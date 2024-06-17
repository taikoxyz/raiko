#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
use risc0_zkvm::guest::env;

use raiko_lib::protocol_instance::ProtocolInstance;
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    consts::VerifierType,
    input::GuestInput,
};
use revm_precompile::zk_op::ZkOperation;
use zk_op::Risc0Operator;

pub mod mem;

pub use mem::*;

fn main() {
    let input: GuestInput = env::read();

    revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Risc0Operator {}));
    revm_precompile::zk_op::ZKVM_OPERATIONS
        .set(Box::new(vec![ZkOperation::Sha256, ZkOperation::Secp256k1]))
        .expect("Failed to set ZkvmOperations");

    let (header, _mpt_node) = TaikoStrategy::build_from(&input).unwrap();
    let pi = ProtocolInstance::new(&input, &header, VerifierType::RISC0)
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
