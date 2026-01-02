#![no_main]
ziskos::entrypoint!(main);

use raiko_lib::{
    builder::calculate_batch_blocks_final_header,
    input::GuestBatchInput,
    proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};

pub fn main() {
    // Read the batch input data from ziskos
    let input_data = ziskos::read_input();

    // Deserialize the batch input using the standard GuestBatchInput format
    let batch_input: GuestBatchInput =
        bincode::deserialize(&input_data).expect("failed to deserialize GuestBatchInput");

    // This executes all transactions and validates state transitions
    let final_blocks = calculate_batch_blocks_final_header(&batch_input);
    
    // Create protocol instance from executed blocks
    let protocol_instance = ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Zisk)
        .expect("failed to build Zisk protocol instance");
    
    // Get the instance hash
    let instance_hash = protocol_instance.instance_hash();
    
    // Commit the protocol instance hash in ZisK format
    // Convert the hash bytes to u32 chunks for ZisK's output format
    let hash_bytes = instance_hash.0;
    for (i, chunk) in hash_bytes.chunks(4).enumerate().take(8) {
        let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        ziskos::set_output(i, value);
    }
}
