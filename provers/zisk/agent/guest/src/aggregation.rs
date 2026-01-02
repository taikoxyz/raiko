//! Aggregates multiple batch proofs (verification handled by the host).

#![no_main]
ziskos::entrypoint!(main);

use raiko_lib::{
    input::ZkAggregationGuestInput,
    primitives::B256,
    protocol_instance::{aggregation_output, words_to_bytes_le},
};

pub fn main() {
    // Read the aggregation input data from ziskos
    let input_data = ziskos::read_input();
    assert!(!input_data.is_empty(), "aggregation input is empty");

    // Deserialize input using the standard ZkAggregationGuestInput format
    let input: ZkAggregationGuestInput =
        bincode::deserialize(&input_data).expect("failed to deserialize ZkAggregationGuestInput");

    assert!(
        !input.block_inputs.is_empty(),
        "aggregation input has no block inputs"
    );
    
    // Use the same aggregation_output function for consistency
    let program_id = B256::from(words_to_bytes_le(&input.image_id));
    let aggregated_output = aggregation_output(program_id, input.block_inputs.clone());
    
    // Commit the aggregation output in ZisK format
    // Convert the output bytes to u32 chunks for ZisK's output format  
    for (i, chunk) in aggregated_output.chunks(4).enumerate().take(8) {
        let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        ziskos::set_output(i, value);
    }
}
