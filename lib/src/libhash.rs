// rust impl of taiko-mono/packages/protocol/contracts/layer1/shasta/libs/LibHashing.sol

use crate::input::shasta::{
    CoreState, Derivation, Proposal, Transition as ShastaTransition, TransitionMetadata,
};

use crate::input::shasta::Checkpoint;
use crate::primitives::keccak::keccak;
use alloy_primitives::{Address, B256, U256};
use reth_primitives::b256;

/// Hash a transition using the same logic as the Solidity implementation
pub fn hash_transition_with_metadata(
    transition: &ShastaTransition,
    metadata: &TransitionMetadata,
) -> B256 {
    // converts designatedProver (Address) to B256 as in Solidity: bytes32(uint256(uint160(_metadata.designatedProver)))
    let designated_prover_b256 = address_to_b256(metadata.designatedProver);
    let prover_b256 = address_to_b256(metadata.actualProver);
    hash_six_values(
        transition.proposalHash,
        transition.parentTransitionHash,
        hash_checkpoint(&transition.checkpoint),
        designated_prover_b256,
        prover_b256,
        metadata.bondProposalHash,
    )
}

/// Hash a checkpoint using the same logic as the Solidity implementation
pub fn hash_checkpoint(checkpoint: &Checkpoint) -> B256 {
    hash_three_values(
        U256::from(checkpoint.blockNumber).into(),
        checkpoint.blockHash,
        checkpoint.stateRoot,
    )
}

/// Returns `keccak256(abi.encode(value0, .., valueN))` - equivalent to Solidity's EfficientHashLib.hash
///
/*
       assembly {
           let m := mload(0x40)
           mstore(m, v0)
           mstore(add(m, 0x20), v1)
           ...
           mstore(add(m, 0xa0), vN)
           result := keccak256(m, 0x20 * N)
       }
*/

/// Returns `keccak256(abi.encode(value0, value1))` - equivalent to Solidity's EfficientHashLib.hash
pub fn hash_two_values(value0: B256, value1: B256) -> B256 {
    hash_values_impl(&[value0, value1])
}

/// Returns `keccak256(abi.encode(value0, value1, value2))`
pub fn hash_three_values(value0: B256, value1: B256, value2: B256) -> B256 {
    hash_values_impl(&[value0, value1, value2])
}

pub fn hash_four_values(value0: B256, value1: B256, value2: B256, value3: B256) -> B256 {
    hash_values_impl(&[value0, value1, value2, value3])
}

pub fn hash_five_values(
    value0: B256,
    value1: B256,
    value2: B256,
    value3: B256,
    value4: B256,
) -> B256 {
    hash_values_impl(&[value0, value1, value2, value3, value4])
}

pub fn hash_six_values(
    value0: B256,
    value1: B256,
    value2: B256,
    value3: B256,
    value4: B256,
    value5: B256,
) -> B256 {
    hash_values_impl(&[value0, value1, value2, value3, value4, value5])
}

fn hash_values_impl(values: &[B256]) -> B256 {
    let mut data = Vec::with_capacity(values.len() * 32);
    for v in values {
        data.extend_from_slice(v.as_slice());
    }
    keccak(&data).into()
}

/// Convert an Address to B256 by zero-padding (equivalent to bytes32(uint256(uint160(address))))
pub fn address_to_b256(address: Address) -> B256 {
    B256::left_padding_from(address.as_slice())
}

const EMPTY_BYTES_HASH: B256 =
    b256!("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");

pub const VERIFY_PROOF_B256: B256 =
    b256!("5645524946595f50524f4f460000000000000000000000000000000000000000");

/// Hash an array of transitions using the same logic as the Solidity implementation
pub fn hash_transitions_array_with_metadata(
    transitions: &[ShastaTransition],
    metadata: &TransitionMetadata,
) -> B256 {
    if transitions.is_empty() {
        return EMPTY_BYTES_HASH;
    }

    // For small arrays (most common case), use direct hashing with length
    if transitions.len() == 1 {
        return hash_two_values(
            U256::from(transitions.len()).into(),
            hash_transition_with_metadata(&transitions[0], metadata),
        );
    }

    if transitions.len() == 2 {
        return hash_three_values(
            U256::from(transitions.len()).into(),
            hash_transition_with_metadata(&transitions[0], metadata),
            hash_transition_with_metadata(&transitions[1], metadata),
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
        let transition_hash = hash_transition_with_metadata(transition, metadata);
        buffer.extend_from_slice(transition_hash.as_slice());
    }

    // Return keccak256 hash of the buffer
    keccak(&buffer).into()
}

// in aggregation, we only need to hash the transitions hash array
pub fn hash_transitions_hash_array_with_metadata(transitions: &[B256]) -> B256 {
    if transitions.is_empty() {
        return EMPTY_BYTES_HASH;
    }

    // For small arrays (most common case), use direct hashing with length
    if transitions.len() == 1 {
        return hash_two_values(U256::from(transitions.len()).into(), transitions[0]);
    }

    if transitions.len() == 2 {
        return hash_three_values(
            U256::from(transitions.len()).into(),
            transitions[0],
            transitions[1],
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
        buffer.extend_from_slice(transition.as_slice());
    }

    // Return keccak256 hash of the buffer
    keccak(&buffer).into()
}

/*
// Hash a proposal using the same logic as the Solidity implementation
unchecked {
        bytes32 packedFields = bytes32(
            (uint256(_proposal.id) << 208) | (uint256(_proposal.timestamp) << 160)
                | (uint256(_proposal.endOfSubmissionWindowTimestamp) << 112)
        );

        return EfficientHashLib.hash(
            packedFields,
            bytes32(uint256(uint160(_proposal.proposer))), // Full 160-bit address
            _proposal.coreStateHash,
            _proposal.derivationHash
        );
    }
*/
pub fn hash_proposal(proposal: &Proposal) -> B256 {
    // Pack the fields as in Solidity, using proper bit shifts and concatenation.
    let packed: U256 = (U256::from(proposal.id) << 208)
        | (U256::from(proposal.timestamp) << 160)
        | (U256::from(proposal.endOfSubmissionWindowTimestamp) << 112);

    // Encode proposer address to B256 by zero-padding its 20 bytes to 32 bytes (uint256(uint160))
    let proposer_b256 = address_to_b256(proposal.proposer);

    hash_four_values(
        packed.into(),
        proposer_b256,
        proposal.coreStateHash,
        proposal.derivationHash,
    )
}

pub fn hash_core_state(core_state: &CoreState) -> B256 {
    hash_six_values(
        U256::from(core_state.nextProposalId).into(),
        U256::from(core_state.lastProposalBlockId).into(),
        U256::from(core_state.lastFinalizedProposalId).into(),
        U256::from(core_state.lastCheckpointTimestamp).into(),
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

pub fn hash_public_input(
    aggregated_proving_hash: B256,
    chain_id: u64,
    verifier_address: Address,
    sgx_instance: Address,
) -> B256 {
    hash_five_values(
        VERIFY_PROOF_B256,
        U256::from(chain_id).into(),
        address_to_b256(verifier_address),
        aggregated_proving_hash,
        address_to_b256(sgx_instance),
    )
}

#[cfg(test)]
mod test {
    use crate::input::shasta::{BlobSlice, Derivation, DerivationSource};
    use reth_primitives::{address, b256};

    use super::*;

    #[test]
    fn test_hash_proposal() {
        let proposal = Proposal {
            id: 3549,
            timestamp: 1761830468,
            endOfSubmissionWindowTimestamp: 0,
            proposer: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
            coreStateHash: b256!(
                "6c3667ff590cbfedc61442117832ab6c43e4ae803e434df81573d4850d9f9522"
            ),
            derivationHash: b256!(
                "85422bfec85e2cb6d5ca9f52858a74b680865c0134c0e29af710d8e01d58898a"
            ),
        };
        let proposal_hash = hash_proposal(&proposal);
        assert_eq!(
            hex::encode(proposal_hash),
            "84d250afffb408d35c42978f6563a32c494ec3a4dc01c5e87e7f3a77c413eaeb"
        );
    }

    #[test]
    fn test_shasta_transition_hash() {
        // Create a transition with fixed test values
        let transition = ShastaTransition {
            proposalHash: b256!("d469fc0c500db1c87cd4fcf0650628cf4be84b03feb29dbca9ce1daee2750274"),
            parentTransitionHash: b256!(
                "66aa40046aa64a8e0a7ecdbbc70fb2c63ebdcb2351e7d0b626ed3cb4f55fb388"
            ),
            checkpoint: Checkpoint {
                blockNumber: 1512,
                blockHash: b256!(
                    "83cf1bb221b330d372ce0fbca82cb060fa028d3f6bfd62a74197789e25ac2b5f"
                ),
                stateRoot: b256!(
                    "63651766d70b5aaf0320fc63421f4d1fdf6fe828514e21e05615e9c2f93c9c7d"
                ),
            },
        };

        let metadata = TransitionMetadata {
            designatedProver: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
            actualProver: address!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
            bondProposalHash: B256::ZERO,
        };
        let single_trans_hash = hash_transition_with_metadata(&transition, &metadata);
        assert_eq!(
            hex::encode(single_trans_hash),
            "8e1bb4b3832a1da199f0d0a7b93e95b8bd96c58045ff3b54d4969dc38a9260da"
        );

        let transition_hash = hash_transitions_array_with_metadata(&[transition], &metadata);
        assert_eq!(
            hex::encode(transition_hash),
            "f84854d6f8b03f973543dc20cf541d78a2a9e25299d6f53b13c8b48e03246a43"
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

        let metadata = TransitionMetadata {
            designatedProver: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
            actualProver: address!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
            bondProposalHash: B256::ZERO,
        };
        // Calculate hash using hashTransitionsArray equivalent
        let result = hash_transitions_array_with_metadata(&transitions, &metadata);
        assert_eq!(
            hex::encode(result),
            "f9e1faec6512a0048465cfee3bb43eadbbfe8fe781ac5eaa4defe841b4e06453"
        );

        // Test individual transition hashes
        let individual_hashes: Vec<String> = transitions
            .iter()
            .map(|t| hex::encode(hash_transition_with_metadata(t, &metadata)))
            .collect();

        assert_eq!(
            individual_hashes[0],
            "1aebd9d633bb849c184d4d7ff14e04b2fcbe9bc93b8a23d22fa56fc944cb19b9"
        );
        assert_eq!(
            individual_hashes[1],
            "a43aeeab9b0b41ea668f7d9ec258a4ef763568801c6f9cff07d75f0d03d8700b"
        );
        assert_eq!(
            individual_hashes[2],
            "262fe70091d0da183a0eee8e271672f387d968c03de653fc01b1e88d930e9d23"
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

    #[test]
    fn test_hash_public_input() {
        let aggregated_proving_hash =
            b256!("b836ee1f972e8bcd4766bede4a9fa5267d8b6ec7cd6088562aca0b07b15f57bc");
        let chain_id = 167001u64;
        let verifier_address = address!("00f9f60C79e38c08b785eE4F1a849900693C6630");
        let public_input_hash = hash_public_input(
            aggregated_proving_hash,
            chain_id,
            verifier_address,
            Address::ZERO,
        );
        assert_eq!(
            hex::encode(public_input_hash),
            "6d0ea3eb338aa3e2d85b21394d3ea426574ab7764726376a5364dee132fcd3d7"
        );
    }
}
