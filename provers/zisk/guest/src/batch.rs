#![no_main]
ziskos::entrypoint!(main);

use raiko_lib::{
    builder::calculate_batch_blocks_final_header,
    input::GuestBatchInput,
    proof_type::ProofType,
    protocol_instance::ProtocolInstance,
};

mod zisk_crypto;
use zisk_crypto::*;

pub fn main() {
    // Read the batch input data from ziskos
    let input_data = ziskos::read_input();
    
    // Handle empty input
    if input_data.is_empty() {
        ziskos::set_output(0, 0xFFFFFFFFu32);
        return;
    }
    
    // Deserialize the batch input using the standard GuestBatchInput format
    let batch_input: GuestBatchInput = match bincode::deserialize(&input_data) {
        Ok(input) => input,
        Err(_) => {
            ziskos::set_output(0, 0xFFFFFFFEu32);
            return;
        }
    };
    
    // Validate input structure
    if batch_input.inputs.is_empty() {
        ziskos::set_output(0, 0xFFFFFFFDu32);
        return;
    }
    
    // This executes all transactions and validates state transitions
    let final_blocks = match std::panic::catch_unwind(|| {
        calculate_batch_blocks_final_header(&batch_input)
    }) {
        Ok(blocks) => blocks,
        Err(_) => {
            ziskos::set_output(0, 0xFFFFFFFCu32);
            return;
        }
    };
    
    // Create protocol instance from executed blocks
    let protocol_instance = match ProtocolInstance::new_batch(&batch_input, final_blocks, ProofType::Zisk) {
        Ok(pi) => pi,
        Err(_) => {
            ziskos::set_output(0, 0xFFFFFFFBu32);
            return;
        }
    };
    
    // Get the instance hash
    let instance_hash = protocol_instance.instance_hash();
    
    // Commit the protocol instance hash in ZisK format
    // Convert the hash bytes to u32 chunks for ZisK's output format
    let hash_bytes = instance_hash.0;
    for (i, chunk) in hash_bytes.chunks(4).enumerate().take(8) {
        if chunk.len() == 4 {
            let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            ziskos::set_output(i, value);
        } else {
            let mut padded = [0u8; 4];
            padded[..chunk.len()].copy_from_slice(chunk);
            let value = u32::from_le_bytes(padded);
            ziskos::set_output(i, value);
        }
    }
}