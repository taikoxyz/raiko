#![no_main]
ziskos::entrypoint!(main);

use serde::{Deserialize, Serialize};

mod zisk_crypto;
use zisk_crypto::*;

/// Simplified batch input for Zisk processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZiskBatchInput {
    pub batch_id: u64,
    pub chain_id: u64,
    pub block_numbers: Vec<u64>,
    pub block_hashes: Vec<[u8; 32]>,
    pub use_emulator_mode: Option<bool>, // Flag to indicate execution mode
}

/// Simplified protocol instance computation
fn compute_batch_hash(input: &ZiskBatchInput) -> [u8; 32] {
    let use_emulator = input.use_emulator_mode.unwrap_or(true);
    
    if use_emulator {
        // Emulator mode: Use simple safe hash computation
        let mut result = [0u8; 32];
        
        // Use batch_id as the primary component of the hash
        let batch_bytes = input.batch_id.to_le_bytes();
        let chain_bytes = input.chain_id.to_le_bytes();
        
        // Fill the result with a simple combination of batch_id and chain_id
        for i in 0..8 {
            result[i] = batch_bytes[i % 8];
            result[i + 8] = chain_bytes[i % 8];
        }
        
        // Add some block data if available
        if !input.block_numbers.is_empty() {
            let block_bytes = input.block_numbers[0].to_le_bytes();
            for i in 0..8 {
                result[i + 16] = block_bytes[i];
            }
        }
        
        // Use block hash if available
        if !input.block_hashes.is_empty() {
            for i in 0..8 {
                result[i + 24] = input.block_hashes[0][i];
            }
        }
        
        result
    } else {
        // Prover mode: Use proper cryptographic hash with Zisk precompiles
        let mut data = Vec::new();
        data.extend_from_slice(&input.batch_id.to_le_bytes());
        data.extend_from_slice(&input.chain_id.to_le_bytes());
        
        // Add block numbers and hashes
        for (block_num, block_hash) in input.block_numbers.iter().zip(input.block_hashes.iter()) {
            data.extend_from_slice(&block_num.to_le_bytes());
            data.extend_from_slice(block_hash);
        }
        
        // Use Zisk's built-in SHA-256 precompile (should work in prover mode)
        sha256(&data)
    }
}

pub fn main() {
    // Read input data
    let input_data = ziskos::read_input();
    
    // Check if we have any input data
    if input_data.is_empty() {
        // If no input, set a default output
        ziskos::set_output(0, 0u32);
        return;
    }
    
    // Try to deserialize the input
    let batch_input: ZiskBatchInput = match bincode::deserialize(&input_data) {
        Ok(input) => input,
        Err(_) => {
            // If deserialization fails, set error output
            ziskos::set_output(0, 0xFFFFFFFFu32);
            return;
        }
    };
    
    // Process the batch
    let batch_hash = compute_batch_hash(&batch_input);
    
    // Set the output - use first 32 bits of the hash as a u32
    let output_value = u32::from_le_bytes([batch_hash[0], batch_hash[1], batch_hash[2], batch_hash[3]]);
    ziskos::set_output(0, output_value);
}