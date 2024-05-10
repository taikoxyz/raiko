#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);

use raiko_lib::protocol_instance::assemble_protocol_instance;
use raiko_lib::protocol_instance::EvidenceType;
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, WrappedHeader},
};
use revm_precompile::zk_op::ZkOperation;
use zk::Risc0Operator;

pub mod mem;

#[cfg(test)]
use harness::*;
pub use mem::*;

fn main() {
    let input: GuestInput = env::read();

    revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Risc0Operator {}));
    revm_precompile::zk_op::ZKVM_OPERATIONS
        .set(Box::new(vec![ZkOperation::Sha256, ZkOperation::Secp256k1]))
        .expect("Failed to set ZkvmOperations");

    let build_result = TaikoStrategy::build_from(&input);

    // TODO: cherry-pick risc0 latest output
    let output = match &build_result {
        Ok((header, _mpt_node)) => {
            let pi = assemble_protocol_instance(&input, header)
                .expect("Failed to assemble protocol instance")
                .instance_hash(EvidenceType::Risc0);
            GuestOutput::Success((
                WrappedHeader {
                    header: header.clone(),
                },
                pi,
            ))
        }
        Err(_) => GuestOutput::Failure,
    };

    env::commit(&output);
}


harness::zk_suits!(
    pub mod tests {
        #[test]
        pub fn test1() {
            assert_eq!(1, 2);
        }
        #[test]
        pub fn test2() {
            assert_eq!(1, 2);
        }
        #[test]
        pub fn test3() {
            assert_eq!(1, 2);
        }
    }
);
