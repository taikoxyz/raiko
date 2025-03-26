#![no_main]
sp1_zkvm::entrypoint!(main);

use raiko_lib::{
    builder::calculate_batch_blocks_final_header, input::GuestBatchInput, proof_type::ProofType,
    protocol_instance::ProtocolInstance, CycleTracker,
};

pub mod sys;
pub use sys::*;

pub fn main() {
    let mut ct = CycleTracker::start("input");
    let input = sp1_zkvm::io::read_vec();
    let batch_input = bincode::deserialize::<GuestBatchInput>(&input).unwrap();
    ct.end();

    ct = CycleTracker::start("calculate_batch_blocks_final_header");
    let final_blocks = calculate_batch_blocks_final_header(&batch_input);
    ct.end();

    ct = CycleTracker::start("batch_instance_hash");
    let pi = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Sp1)
        .unwrap()
        .instance_hash();
    ct.end();

    sp1_zkvm::io::commit(&pi.0);
}

harness::zk_suits!(
    pub mod tests {
        use reth_primitives::U256;
        use std::str::FromStr;
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
            let hash = reth_primitives::B256::from_str(
                "daf5a779ae972f972197303d7b574746c7ef83eadac0f2791ad23db92e4c8e53",
            )
            .unwrap();
            signature.recover_signer(hash).unwrap();
        }
    }
);
