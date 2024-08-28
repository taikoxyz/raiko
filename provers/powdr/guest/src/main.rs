#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
use powdr_riscv_runtime as powdr;
use raiko_lib::{
    builder::calculate_block_header, consts::VerifierType, input::GuestInput,
    protocol_instance::ProtocolInstance,
};
use revm_precompile::zk_op::ZkOperation;
use zk_op::Risc0Operator;

pub mod mem;
pub use mem::*;

const INPUT_FD: u32 = 42;
const OUTPUT_FD: u32 = 43;

fn main() {
    let input: GuestInput = powdr::io::read(INPUT_FD);

    revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Risc0Operator {}));
    revm_precompile::zk_op::ZKVM_OPERATIONS
        .set(Box::new(vec![ZkOperation::Sha256, ZkOperation::Secp256k1]))
        .expect("Failed to set ZkvmOperations");

    let header = calculate_block_header(&input);
    let pi = ProtocolInstance::new(&input, &header, VerifierType::RISC0)
        .unwrap()
        .instance_hash();

    powdr::io::write(OUTPUT_FD, &pi);
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
