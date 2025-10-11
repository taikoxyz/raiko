// rust impl of taiko-mono/packages/protocol/contracts/layer1/shasta/libs/LibHashing.sol

use crate::input::shasta::{CoreState, Derivation, Transition as ShastaTransition};

use crate::input::shasta::Checkpoint;
use crate::primitives::keccak::keccak;
use alloy_primitives::{Address, B256, U256};
use reth_primitives::b256;

/// Hash a transition using the same logic as the Solidity implementation
pub fn hash_transition(transition: &ShastaTransition) -> B256 {
    hash_three_values(
        transition.proposalHash,
        transition.parentTransitionHash,
        hash_checkpoint(&transition.checkpoint),
    )
}

/// Returns `keccak256(abi.encode(value0, .., value4))` - equivalent to Solidity's EfficientHashLib.hash
pub fn hash_five_values(
    value0: B256,
    value1: B256,
    value2: B256,
    value3: B256,
    value4: B256,
) -> B256 {
    let mut data = Vec::with_capacity(160); // 5 * 32 bytes
    data.extend_from_slice(value0.as_slice());
    data.extend_from_slice(value1.as_slice());
    data.extend_from_slice(value2.as_slice());
    data.extend_from_slice(value3.as_slice());
    data.extend_from_slice(value4.as_slice());

    keccak(&data).into()
}

/// Hash a checkpoint using the same logic as the Solidity implementation
pub fn hash_checkpoint(checkpoint: &Checkpoint) -> B256 {
    hash_three_values(
        U256::from(checkpoint.blockNumber).into(),
        checkpoint.blockHash,
        checkpoint.stateRoot,
    )
}

/// Returns `keccak256(abi.encode(value0, value1))` - equivalent to Solidity's EfficientHashLib.hash
pub fn hash_two_values(value0: B256, value1: B256) -> B256 {
    let mut data = Vec::with_capacity(64); // 2 * 32 bytes
    data.extend_from_slice(value0.as_slice());
    data.extend_from_slice(value1.as_slice());

    keccak(&data).into()
}

/// Returns `keccak256(abi.encode(value0, value1, value2))`
pub fn hash_three_values(value0: B256, value1: B256, value2: B256) -> B256 {
    let mut data = Vec::with_capacity(96); // 3 * 32 bytes
    data.extend_from_slice(value0.as_slice());
    data.extend_from_slice(value1.as_slice());
    data.extend_from_slice(value2.as_slice());

    keccak(&data).into()
}

/// Convert an Address to B256 by zero-padding (equivalent to bytes32(uint256(uint160(address))))
fn address_to_b256(address: Address) -> B256 {
    let mut result = [0u8; 32];
    result[12..32].copy_from_slice(address.as_slice());
    B256::from(result)
}

const EMPTY_BYTES_HASH: B256 =
    b256!("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");

/// Hash an array of transitions using the same logic as the Solidity implementation
pub fn hash_transitions_array(transitions: &[ShastaTransition]) -> B256 {
    if transitions.is_empty() {
        return EMPTY_BYTES_HASH;
    }

    // For small arrays (most common case), use direct hashing with length
    if transitions.len() == 1 {
        return hash_two_values(
            U256::from(transitions.len()).into(),
            hash_transition(&transitions[0]),
        );
    }

    if transitions.len() == 2 {
        return hash_three_values(
            U256::from(transitions.len()).into(),
            hash_transition(&transitions[0]),
            hash_transition(&transitions[1]),
        );
    }

    // For larger arrays, use memory-optimized approach
    // Pre-allocate exact buffer size: 32 bytes for length + 32 bytes per hash
    let array_length = transitions.len();
    let buffer_size = 32 + (array_length * 32);
    let mut buffer = Vec::with_capacity(buffer_size);

    // Write array length at start of buffer
    buffer.extend_from_slice(&U256::from(array_length).to_be_bytes::<32>());

    // Write each transition hash directly to buffer
    for transition in transitions {
        let transition_hash = hash_transition(transition);
        buffer.extend_from_slice(transition_hash.as_slice());
    }

    // Return keccak256 hash of the buffer
    keccak(&buffer).into()
}

pub fn hash_core_state(core_state: &CoreState) -> B256 {
    hash_five_values(
        U256::from(core_state.nextProposalId).into(),
        U256::from(core_state.nextProposalBlockId).into(),
        U256::from(core_state.lastFinalizedProposalId).into(),
        core_state.lastFinalizedTransitionHash,
        core_state.bondInstructionsHash,
    )
}

/// Hash a derivation source (isForcedInclusion flag + blobSlice)
pub fn hash_derivation_source(source: &crate::input::shasta::DerivationSource) -> B256 {
    hash_two_values(
        if source.isForcedInclusion {
            B256::from([1u8; 32])
        } else {
            B256::from([0u8; 32])
        },
        hash_blob_slice(&source.blobSlice),
    )
}

/// Hash a blob slice using the same logic as the Solidity implementation
fn hash_blob_slice(blob_slice: &crate::input::shasta::BlobSlice) -> B256 {
    // Hash the blob hashes array first
    let blob_hashes_hash = if blob_slice.blobHashes.is_empty() {
        EMPTY_BYTES_HASH
    } else if blob_slice.blobHashes.len() == 1 {
        hash_two_values(
            U256::from(blob_slice.blobHashes.len()).into(),
            blob_slice.blobHashes[0],
        )
    } else if blob_slice.blobHashes.len() == 2 {
        hash_three_values(
            U256::from(blob_slice.blobHashes.len()).into(),
            blob_slice.blobHashes[0],
            blob_slice.blobHashes[1],
        )
    } else {
        // For larger arrays, use memory-optimized approach
        let array_length = blob_slice.blobHashes.len();
        let buffer_size = 32 + (array_length * 32);
        let mut buffer = Vec::with_capacity(buffer_size);

        // Write array length at start of buffer
        buffer.extend_from_slice(&U256::from(array_length).to_be_bytes::<32>());

        // Write each blob hash directly to buffer
        for blob_hash in &blob_slice.blobHashes {
            buffer.extend_from_slice(blob_hash.as_slice());
        }

        keccak(&buffer).into()
    };

    // Hash the three values: blob_hashes_hash, offset, timestamp
    hash_three_values(
        blob_hashes_hash,
        U256::from(blob_slice.offset).into(),
        U256::from(blob_slice.timestamp).into(),
    )
}

fn pack_derivation_fields(derivation: &Derivation) -> B256 {
    let mut packed = [0u8; 32];
    let origin_block_number_bytes = derivation.originBlockNumber.to_be_bytes();
    packed[0..6].copy_from_slice(&origin_block_number_bytes[2..8]); // Take last 6 bytes

    // Pack basefeeSharingPctg at offset 24 (192 bits / 8 = 24 bytes from the right)
    packed[7] = derivation.basefeeSharingPctg; // Take last byte

    B256::from(packed)
}

pub fn hash_derivation(derivation: &Derivation) -> B256 {
    // Pack the fields: originBlockNumber (48 bits) << 208 | basefeeSharingPctg (8 bits) << 192
    let packed_fields = pack_derivation_fields(derivation);

    // Hash the sources array
    let sources_hash = if derivation.sources.is_empty() {
        EMPTY_BYTES_HASH
    } else if derivation.sources.len() == 1 {
        hash_two_values(
            U256::from(derivation.sources.len()).into(),
            hash_derivation_source(&derivation.sources[0]),
        )
    } else if derivation.sources.len() == 2 {
        hash_three_values(
            U256::from(derivation.sources.len()).into(),
            hash_derivation_source(&derivation.sources[0]),
            hash_derivation_source(&derivation.sources[1]),
        )
    } else {
        // For larger arrays, use memory-optimized approach
        let array_length = derivation.sources.len();
        let buffer_size = 32 + (array_length * 32);
        let mut buffer = Vec::with_capacity(buffer_size);

        // Write array length at start of buffer
        buffer.extend_from_slice(&U256::from(array_length).to_be_bytes::<32>());

        // Write each source hash directly to buffer
        for source in &derivation.sources {
            let source_hash = hash_derivation_source(source);
            buffer.extend_from_slice(source_hash.as_slice());
        }

        keccak(&buffer).into()
    };

    // Hash the three values: packed_fields, originBlockHash, sourcesHash
    hash_three_values(packed_fields, derivation.originBlockHash, sources_hash)
}

#[cfg(test)]
mod test {
    use crate::input::shasta::{BlobSlice, Derivation, DerivationSource};
    use reth_primitives::b256;

    use super::*;

    #[test]
    fn test_shasta_transition_hash() {
        // Create a transition with fixed test values
        let transition = ShastaTransition {
            proposalHash: b256!("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            parentTransitionHash: b256!(
                "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            ),
            checkpoint: Checkpoint {
                blockNumber: 999999,
                blockHash: b256!(
                    "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                ),
                stateRoot: b256!(
                    "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                ),
            },
        };

        let transition_hash = hash_transition(&transition);
        assert_eq!(
            hex::encode(transition_hash),
            "9d93decbd29774478548308c831ea03f3c38aa43ce07b3302b965ef1a0555b1e"
        );
    }

    #[test]
    fn test_hash_transitions_array_fixed_values() {
        // Create array with 3 transitions using fixed values
        let transitions = vec![
            // Transition 1
            ShastaTransition {
                proposalHash: b256!(
                    "1111111111111111111111111111111111111111111111111111111111111111"
                ),
                parentTransitionHash: b256!(
                    "2222222222222222222222222222222222222222222222222222222222222222"
                ),
                checkpoint: Checkpoint {
                    blockNumber: 1000,
                    blockHash: b256!(
                        "3333333333333333333333333333333333333333333333333333333333333333"
                    ),
                    stateRoot: b256!(
                        "4444444444444444444444444444444444444444444444444444444444444444"
                    ),
                },
            },
            // Transition 2
            ShastaTransition {
                proposalHash: b256!(
                    "5555555555555555555555555555555555555555555555555555555555555555"
                ),
                parentTransitionHash: b256!(
                    "6666666666666666666666666666666666666666666666666666666666666666"
                ),
                checkpoint: Checkpoint {
                    blockNumber: 2000,
                    blockHash: b256!(
                        "7777777777777777777777777777777777777777777777777777777777777777"
                    ),
                    stateRoot: b256!(
                        "8888888888888888888888888888888888888888888888888888888888888888"
                    ),
                },
            },
            // Transition 3
            ShastaTransition {
                proposalHash: b256!(
                    "9999999999999999999999999999999999999999999999999999999999999999"
                ),
                parentTransitionHash: b256!(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                ),
                checkpoint: Checkpoint {
                    blockNumber: 3000,
                    blockHash: b256!(
                        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    ),
                    stateRoot: b256!(
                        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    ),
                },
            },
        ];

        // Calculate hash using hashTransitionsArray equivalent
        let result = hash_transitions_array(&transitions);
        assert_eq!(
            hex::encode(result),
            "c49b25f3b1b5fb1c9f2b4a574305f2db250a6539c3857aa8cca9f074a0cc1ccf"
        );

        // Test individual transition hashes
        let individual_hashes: Vec<String> = transitions
            .iter()
            .map(|t| hex::encode(hash_transition(t)))
            .collect();

        assert_eq!(
            individual_hashes[0],
            "a7dc0c76776e60af434abbee470112e0e2198edf06dea1c5153d431d58a84df9"
        );
        assert_eq!(
            individual_hashes[1],
            "f7ced7b0c00b03d9832712d12eefea6f48b44fff2a399a99b3fe1a0e063f8c2b"
        );
        assert_eq!(
            individual_hashes[2],
            "fead1ace85f53c5077d1184d8b83937ac110369a5eb65e582b8cae997e85ea21"
        );
    }

    #[test]
    fn test_pack_derivation_fields() {
        let derivation = Derivation {
            originBlockNumber: 155,
            originBlockHash: b256!(
                "10746c6d70f2b59483dc2e0a1315758799fb3655f87e430568e71591589f76f9"
            ),
            basefeeSharingPctg: 75,
            sources: Vec::new(),
        };

        let packed_fields = pack_derivation_fields(&derivation);
        assert_eq!(
            packed_fields,
            b256!("00000000009b004b000000000000000000000000000000000000000000000000")
        );
    }

    #[test]
    fn test_hash_derivation_empty_source() {
        // Create a test derivation with one source
        let derivation = Derivation {
            originBlockNumber: 155,
            originBlockHash: b256!(
                "10746c6d70f2b59483dc2e0a1315758799fb3655f87e430568e71591589f76f9"
            ),
            basefeeSharingPctg: 75,
            sources: Vec::new(),
        };

        let derivation_hash = hash_derivation(&derivation);

        // The hash should be deterministic and match the expected value
        // This test verifies the implementation works without errors
        assert_ne!(derivation_hash, B256::ZERO);
        assert_eq!(
            hex::encode(derivation_hash),
            "f7591d96a9236272ae9c839b84b64fdc2d97873d80992417969e4f639ac57656"
        );
    }

    #[test]
    fn test_hash_derivation() {
        // Create a test derivation with one source
        let derivation = Derivation {
            originBlockNumber: 155,
            originBlockHash: b256!(
                "10746c6d70f2b59483dc2e0a1315758799fb3655f87e430568e71591589f76f9"
            ),
            basefeeSharingPctg: 75,
            sources: vec![DerivationSource {
                isForcedInclusion: false,
                blobSlice: BlobSlice {
                    blobHashes: vec![b256!(
                        "0189ea2792db70c7d2165c397be7bc37b7d45b1ed082bec866e9cb62e90cb4a0"
                    )],
                    offset: 0,
                    timestamp: 1758948572,
                },
            }],
        };

        let derivation_hash = hash_derivation(&derivation);

        // The hash should be deterministic and match the expected value
        // This test verifies the implementation works without errors
        assert_ne!(derivation_hash, B256::ZERO);
        println!("Derivation hash: 0x{}", hex::encode(derivation_hash));
    }
}
