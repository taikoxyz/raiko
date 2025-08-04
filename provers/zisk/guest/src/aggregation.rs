#![no_main]
ziskos::entrypoint!(main);

use serde::{Deserialize, Serialize};

mod zisk_crypto;
use zisk_crypto::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZiskAggregationInput {
    pub image_id: [u32; 8],
    pub block_inputs: Vec<[u8; 32]>,
}

fn compute_aggregation_hash(input: &ZiskAggregationInput) -> [u8; 32] {
    let mut data = Vec::new();
    
    // Add image ID
    for word in input.image_id.iter() {
        data.extend_from_slice(&word.to_le_bytes());
    }
    
    // Add all block inputs
    for block_input in input.block_inputs.iter() {
        data.extend_from_slice(block_input);
    }
    
    // Use Zisk's built-in SHA-256 precompile for aggregation
    sha256(&data)
}

pub fn main() {
    // Read input data
    let input_data = ziskos::read_input();
    let aggregation_input: ZiskAggregationInput = bincode::deserialize(&input_data).unwrap();
    
    // Process the aggregation using Zisk's native crypto operations
    let aggregation_hash = compute_aggregation_hash(&aggregation_input);
    
    // Set the output - Zisk's set_output takes (id: usize, value: u32)
    // For now, we'll output the first 32 bits of the hash as a u32
    let output_value = u32::from_le_bytes([aggregation_hash[0], aggregation_hash[1], aggregation_hash[2], aggregation_hash[3]]);
    ziskos::set_output(0, output_value);
}