#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
// harness::entrypoint!(main, tests);

use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, WrappedHeader},
    protocol_instance::{assemble_protocol_instance, EvidenceType},
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

    let build_result = TaikoStrategy::build_from(&input);

    let output = match &build_result {
        Ok((header, _mpt_node)) => {
            let pi = assemble_protocol_instance(&input, header)
                .expect("Failed to assemble protocol instance")
                .instance_hash(EvidenceType::Succinct);
            GuestOutput::Success((
                WrappedHeader {
                    header: header.clone(),
                },
                pi,
            ))
        }
        Err(_) => GuestOutput::Failure,
    };

    sp1_zkvm::io::commit(&output);
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
