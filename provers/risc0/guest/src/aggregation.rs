#![no_main]
harness::entrypoint!(main);
use raiko_lib::input::ZkAggregationGuestInput;
use risc0_zkvm::guest::env;

fn main() {
    // Read the aggregation input
    let input = sp1_zkvm::io::read::<ZkAggregationGuestInput>();

    // Verify the block proofs.
    for block_input in input.block_inputs {
        env::verify(input.image_id, &block_input).unwrap();
    }

    // The aggregation output
    env::commit(&aggregation_output(&words_to_bytes_le(input.image_id), input.block_inputs));
}
