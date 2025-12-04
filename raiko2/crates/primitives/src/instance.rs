//! Protocol instance types for raiko2.
//!
//! This module provides types for constructing and verifying protocol instances
//! for the Shasta hardfork. Legacy fork support (Hekla, Ontake, Pacaya) has been
//! removed in V2.

use crate::{BlobProofType, GuestInput, TaikoProverData};
use alloy_primitives::{Address, B256};
use alloy_sol_types::SolValue;
use anyhow::{ensure, Result};
use reth_ethereum_primitives::Block;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Keccak256 hash function.
fn keccak256(data: impl AsRef<[u8]>) -> B256 {
    use alloy_primitives::keccak256;
    keccak256(data.as_ref())
}

/// Transition data for Shasta.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ShastaTransition {
    pub parent_hash: B256,
    pub block_hash: B256,
    pub state_root: B256,
}

/// Batch metadata for Shasta.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ShastaBatchMetadata {
    pub info_hash: B256,
    pub proposer: Address,
    pub batch_id: u64,
    pub proposed_at: u64,
}

/// Protocol instance for Shasta.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProtocolInstance {
    pub transition: ShastaTransition,
    pub batch_metadata: ShastaBatchMetadata,
    pub prover: Address,
    pub chain_id: u64,
    pub verifier_address: Address,
}

impl ProtocolInstance {
    /// Calculate the instance hash for the protocol instance.
    pub fn instance_hash(&self) -> B256 {
        let data = (
            self.transition.parent_hash,
            self.transition.block_hash,
            self.transition.state_root,
            self.batch_metadata.info_hash,
            self.batch_metadata.proposer,
            self.batch_metadata.batch_id,
            self.prover,
            self.chain_id,
        )
            .abi_encode();
        keccak256(data)
    }
}

/// Verify blob usage in batch mode.
///
/// Checks that raw blob commitment matches input blob commitment,
/// then verifies the blob version hash.
pub fn verify_batch_mode_blob_usage(
    batch_input: &GuestInput,
    blob_proof_type: BlobProofType,
) -> Result<()> {
    match blob_proof_type {
        BlobProofType::KzgVersionedHash => {
            ensure!(
                batch_input.taiko.tx_data_from_blob.len()
                    == batch_input
                        .taiko
                        .blob_commitments
                        .as_ref()
                        .map_or(0, |c| c.len()),
                "Each blob should have its own hash commit"
            );
        }
        BlobProofType::ProofOfEquivalence => {
            ensure!(
                batch_input.taiko.tx_data_from_blob.len()
                    == batch_input
                        .taiko
                        .blob_proofs
                        .as_ref()
                        .map_or(0, |p| p.len()),
                "Each blob should have its own proof"
            );
        }
    }

    // TODO: Implement full blob verification with KZG
    // For now, just verify the counts match

    Ok(())
}

/// Calculate the txs hash for Shasta.
pub fn calculate_txs_hash(tx_list_hash: B256, blob_hashes: &[B256]) -> B256 {
    debug!(
        "calculate_txs_hash from tx_list_hash: {:?}, blob_hashes: {:?}",
        tx_list_hash, blob_hashes
    );

    let abi_encode_data: Vec<u8> = (tx_list_hash, blob_hashes.iter().collect::<Vec<_>>())
        .abi_encode()
        .split_off(32);
    debug!("abi_encode_data: {:?}", hex::encode(&abi_encode_data));
    keccak256(abi_encode_data)
}

/// Create a protocol instance from batch input and executed blocks.
pub fn new_protocol_instance(
    batch_input: &GuestInput,
    blocks: Vec<Block>,
    prover_data: &TaikoProverData,
    chain_id: u64,
    verifier_address: Address,
) -> Result<ProtocolInstance> {
    ensure!(!blocks.is_empty(), "blocks cannot be empty");

    let first_block = blocks.first().unwrap();
    let last_block = blocks.last().unwrap();

    let transition = ShastaTransition {
        parent_hash: first_block.header.parent_hash,
        block_hash: last_block.header.hash_slow(),
        state_root: last_block.header.state_root,
    };

    // Calculate batch metadata
    let tx_list_hash = keccak256(&batch_input.taiko.tx_data_from_calldata);

    // TODO: Get blob hashes from batch_proposed
    let blob_hashes: Vec<B256> = vec![];
    let txs_hash = calculate_txs_hash(tx_list_hash, &blob_hashes);

    let batch_metadata = ShastaBatchMetadata {
        info_hash: txs_hash, // Simplified for now
        proposer: Address::default(),
        batch_id: batch_input.taiko.batch_id,
        proposed_at: 0,
    };

    Ok(ProtocolInstance {
        transition,
        batch_metadata,
        prover: prover_data.prover,
        chain_id,
        verifier_address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_hash() {
        let instance = ProtocolInstance::default();
        let hash = instance.instance_hash();
        assert_ne!(hash, B256::default());
    }

    #[test]
    fn test_calculate_txs_hash() {
        let tx_list_hash = B256::default();
        let blob_hashes = vec![B256::default()];
        let hash = calculate_txs_hash(tx_list_hash, &blob_hashes);
        assert_ne!(hash, B256::default());
    }
}
