#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
// harness::entrypoint!(main, tests);

use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    consts::VerifierType,
    input::{GuestInput},
    protocol_instance::ProtocolInstance,
};
use revm_precompile::zk_op::ZkOperation;
use zk_op::Sp1Operator;

pub mod mem;
pub use mem::*;

pub fn main() {
    let input = sp1_zkvm::io::read::<GuestInput>();

    revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Sp1Operator {}));
    revm_precompile::zk_op::ZKVM_OPERATIONS
        .set(Box::new(vec![
            ZkOperation::Bn128Add,
            ZkOperation::Bn128Mul,
            ZkOperation::Secp256k1,
        ]))
        .expect("Failed to set ZkvmOperations");

    let (header, _mpt_node) = TaikoStrategy::build_from(&input).unwrap();
    let pi = ProtocolInstance::new(&input, &header, VerifierType::SP1)
        .unwrap()
        .instance_hash();

    sp1_zkvm::io::commit(&pi);
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
