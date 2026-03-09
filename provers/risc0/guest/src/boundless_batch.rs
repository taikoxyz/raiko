#![no_main]
harness::entrypoint!(main, tests, zk_op::tests);
use bincode;
use raiko_lib::{
    builder::calculate_batch_blocks_final_header, input::GuestBatchInput, proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};
use risc0_zkvm::guest::env;

fn main() {
    let input_buf: Vec<u8> = env::read_frame();
    let mut batch_input: GuestBatchInput =
        bincode::deserialize::<GuestBatchInput>(&input_buf).unwrap();

    let final_blocks = calculate_batch_blocks_final_header(&mut batch_input);
    let pi = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Risc0)
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
