#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
use std::str::FromStr;

use raiko_lib::{
    consts::VerifierType,
    builder::calculate_block_header, input::GuestInput,
    protocol_instance::ProtocolInstance,
    primitives::B256,
};
use reth_primitives::{B512, U256};
use revm_precompile::zk_op::ZkOperation;
use zk_op::Sp1Operator;

pub mod mem;
pub use mem::*;

pub fn main() {
    println!("cycle-tracker-start: input");
    let input = sp1_zkvm::io::read_vec();
    let input = bincode::deserialize::<GuestInput>(&input).unwrap();
    println!("cycle-tracker-end: input");

    // revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Sp1Operator {}));
    // revm_precompile::zk_op::ZKVM_OPERATIONS
    //     .set(Box::new(vec![
    //         ZkOperation::Bn128Add,
    //         ZkOperation::Bn128Mul,
    //         //already patched with https://github.com/CeciliaZ030/rust-secp256k1
    //         // ZkOperation::Secp256k1, 
    //     ]))
    //     .expect("Failed to set ZkvmOperations");



    let signature = reth_primitives::Signature {
        r: U256::from_str(
            "18515461264373351373200002665853028612451056578545711640558177340181847433846",
        )
        .unwrap(),
        s: U256::from_str(
            "46948507304638947509940763649030358759909902576025900602547168820602576006531",
        )
        .unwrap(),
        odd_y_parity: false,
    };
    let hash =
        reth_primitives::B256::from_str("daf5a779ae972f972197303d7b574746c7ef83eadac0f2791ad23db92e4c8e53")
            .unwrap();

    let signer = signature.recover_signer(hash).unwrap();



    // let header = calculate_block_header(&input);
    // let pi = ProtocolInstance::new(&input, &header, VerifierType::SP1)
    //     .unwrap()
    //     .instance_hash();

    let pi = B256::random();

    sp1_zkvm::io::commit(&pi.0);
}

harness::zk_suits!(
    pub mod tests {
        use std::str::FromStr;
        use reth_primitives::U256;
        #[test]
        pub fn test_build_from_mock_input() {
            // Todo: impl mock input for static unit test
            assert_eq!(1, 1);
            
        }

        pub fn test_signature() {
            let signature = reth_primitives::Signature {
                r: U256::from_str(
                    "18515461264373351373200002665853028612451056578545711640558177340181847433846",
                )
                .unwrap(),
                s: U256::from_str(
                    "46948507304638947509940763649030358759909902576025900602547168820602576006531",
                )
                .unwrap(),
                odd_y_parity: false,
            };
            let hash =
                reth_primitives::B256::from_str("daf5a779ae972f972197303d7b574746c7ef83eadac0f2791ad23db92e4c8e53")
                    .unwrap();
            let signer = signature.recover_signer(hash).unwrap();
        }
    }
);
