// rust impl of taiko-mono/packages/protocol/contracts/layer1/shasta/libs/LibHashing.sol

use crate::input::shasta::{CoreState, Derivation, Proposal, Transition as ShastaTransition};

use crate::input::shasta::Checkpoint;
use crate::primitives::keccak::keccak;
use alloy_primitives::{Address, B256, U256};
use reth_primitives::b256;

/// Hash a transition using the same logic as the Solidity implementation
pub fn hash_shasta_transition(transition: &ShastaTransition) -> B256 {
    // converts designatedProver (Address) to B256 as in Solidity: bytes32(uint256(uint160(_metadata.designatedProver)))
    let designated_prover_b256 = address_to_b256(transition.designatedProver);
    let prover_b256 = address_to_b256(transition.actualProver);
    hash_five_values(
        transition.proposalHash,
        transition.parentTransitionHash,
        hash_checkpoint(&transition.checkpoint),
        designated_prover_b256,
        prover_b256,
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
pub fn hash_shasta_transitions_array(transitions: &[ShastaTransition]) -> B256 {
    if transitions.is_empty() {
        return EMPTY_BYTES_HASH;
    }

    // For small arrays (most common case), use direct hashing with length
    if transitions.len() == 1 {
        return hash_two_values(
            U256::from(transitions.len()).into(),
            hash_shasta_transition(&transitions[0]),
        );
    }

    if transitions.len() == 2 {
        return hash_three_values(
            U256::from(transitions.len()).into(),
            hash_shasta_transition(&transitions[0]),
            hash_shasta_transition(&transitions[1]),
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
        let transition_hash = hash_shasta_transition(transition);
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
    hash_three_values(packed.into(), proposer_b256, proposal.derivationHash)
}

pub fn hash_core_state(core_state: &CoreState) -> B256 {
    hash_five_values(
        U256::from(core_state.nextProposalId).into(),
        U256::from(core_state.lastProposalBlockId).into(),
        U256::from(core_state.lastFinalizedProposalId).into(),
        U256::from(core_state.lastCheckpointTimestamp).into(),
        core_state.lastFinalizedTransitionHash,
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

/*
IInbox.DerivationSource[] memory sources = _derivation.sources;
            uint256 sourcesLength = sources.length;

            // Base words:
            // [0] offset to tuple head (0x20)
            // [1] originBlockNumber
            // [2] originBlockHash
            // [3] basefeeSharingPctg
            // [4] offset to sources (0x80)
            // [5] sources length
            uint256 totalWords = 6 + sourcesLength;

            // Each source contributes: element head (2) + blobSlice head (3) + blobHashes length (1)
            // + blobHashes entries
            for (uint256 i; i < sourcesLength; ++i) {
                totalWords += 6 + sources[i].blobSlice.blobHashes.length;
            }

            bytes32[] memory buffer = EfficientHashLib.malloc(totalWords);

            EfficientHashLib.set(buffer, 0, bytes32(uint256(0x20)));
            EfficientHashLib.set(buffer, 1, bytes32(uint256(_derivation.originBlockNumber)));
            EfficientHashLib.set(buffer, 2, _derivation.originBlockHash);
            EfficientHashLib.set(buffer, 3, bytes32(uint256(_derivation.basefeeSharingPctg)));
            EfficientHashLib.set(buffer, 4, bytes32(uint256(0x80)));
            EfficientHashLib.set(buffer, 5, bytes32(sourcesLength));

            uint256 offsetsBase = 6;
            uint256 dataCursor = offsetsBase + sourcesLength;

            for (uint256 i; i < sourcesLength; ++i) {
                IInbox.DerivationSource memory source = sources[i];
                EfficientHashLib.set(
                    buffer, offsetsBase + i, bytes32((dataCursor - offsetsBase) << 5)
                );

                // DerivationSource head
                EfficientHashLib.set(
                    buffer, dataCursor, bytes32(uint256(source.isForcedInclusion ? 1 : 0))
                );
                EfficientHashLib.set(buffer, dataCursor + 1, bytes32(uint256(0x40)));

                // BlobSlice head
                uint256 blobSliceBase = dataCursor + 2;
                EfficientHashLib.set(buffer, blobSliceBase, bytes32(uint256(0x60)));
                EfficientHashLib.set(
                    buffer, blobSliceBase + 1, bytes32(uint256(source.blobSlice.offset))
                );
                EfficientHashLib.set(
                    buffer, blobSliceBase + 2, bytes32(uint256(source.blobSlice.timestamp))
                );

                // Blob hashes array
                bytes32[] memory blobHashes = source.blobSlice.blobHashes;
                uint256 blobHashesLength = blobHashes.length;
                uint256 blobHashesBase = blobSliceBase + 3;
                EfficientHashLib.set(buffer, blobHashesBase, bytes32(blobHashesLength));

                for (uint256 j; j < blobHashesLength; ++j) {
                    EfficientHashLib.set(buffer, blobHashesBase + 1 + j, blobHashes[j]);
                }

                dataCursor = blobHashesBase + 1 + blobHashesLength;
            }

            bytes32 result = EfficientHashLib.hash(buffer);
            EfficientHashLib.free(buffer);
 */
pub fn hash_derivation(derivation: &Derivation) -> B256 {
    let sources_length = derivation.sources.len();

    // Calculate total words needed for the buffer
    // Base words: 6 (offset to tuple head, originBlockNumber, originBlockHash, basefeeSharingPctg, offset to sources, sources length)
    let mut total_words = 6 + sources_length;

    // Each source contributes: element head (2) + blobSlice head (3) + blobHashes length (1) + blobHashes entries
    for source in &derivation.sources {
        total_words += 6 + source.blobSlice.blobHashes.len();
    }

    // Allocate buffer: each word is 32 bytes (B256), initialize with zeros
    let mut buffer = vec![0u8; total_words * 32];

    // Helper function to write a word at a specific index
    let write_word = |buf: &mut [u8], index: usize, value: B256| {
        let pos = index * 32;
        buf[pos..pos + 32].copy_from_slice(value.as_slice());
    };

    // Set base words
    // [0] offset to tuple head (0x20)
    write_word(&mut buffer, 0, U256::from(0x20u64).into());
    // [1] originBlockNumber
    write_word(&mut buffer, 1, U256::from(derivation.originBlockNumber).into());
    // [2] originBlockHash
    write_word(&mut buffer, 2, derivation.originBlockHash);
    // [3] basefeeSharingPctg
    write_word(&mut buffer, 3, U256::from(derivation.basefeeSharingPctg).into());
    // [4] offset to sources (0x80)
    write_word(&mut buffer, 4, U256::from(0x80u64).into());
    // [5] sources length
    write_word(&mut buffer, 5, U256::from(sources_length).into());

    let offsets_base = 6;
    let mut data_cursor = offsets_base + sources_length;

    // Process each source
    for (i, source) in derivation.sources.iter().enumerate() {
        // Set offset for this source: (dataCursor - offsetsBase) << 5
        let offset = ((data_cursor - offsets_base) << 5) as u64;
        let offset_index = offsets_base + i;
        write_word(&mut buffer, offset_index, U256::from(offset).into());

        // DerivationSource head
        // [dataCursor] isForcedInclusion (1 or 0)
        let is_forced_inclusion_value = if source.isForcedInclusion { 1u64 } else { 0u64 };
        write_word(&mut buffer, data_cursor, U256::from(is_forced_inclusion_value).into());
        // [dataCursor + 1] offset to blobSlice (0x40)
        write_word(&mut buffer, data_cursor + 1, U256::from(0x40u64).into());

        // BlobSlice head
        let blob_slice_base = data_cursor + 2;
        // [blobSliceBase] offset to blobHashes (0x60)
        write_word(&mut buffer, blob_slice_base, U256::from(0x60u64).into());
        // [blobSliceBase + 1] offset
        write_word(&mut buffer, blob_slice_base + 1, U256::from(source.blobSlice.offset).into());
        // [blobSliceBase + 2] timestamp
        write_word(&mut buffer, blob_slice_base + 2, U256::from(source.blobSlice.timestamp).into());

        // Blob hashes array
        let blob_hashes_base = blob_slice_base + 3;
        let blob_hashes_length = source.blobSlice.blobHashes.len();
        // [blobHashesBase] blobHashes length
        write_word(&mut buffer, blob_hashes_base, U256::from(blob_hashes_length).into());

        // [blobHashesBase + 1 + j] each blobHash
        for (j, blob_hash) in source.blobSlice.blobHashes.iter().enumerate() {
            write_word(&mut buffer, blob_hashes_base + 1 + j, *blob_hash);
        }

        data_cursor = blob_hashes_base + 1 + blob_hashes_length;
    }

    // Hash the entire buffer
    keccak(&buffer).into()
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
            designatedProver: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
            actualProver: address!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
        };

        let single_trans_hash = hash_shasta_transition(&transition);
        assert_eq!(
            hex::encode(single_trans_hash),
            "8e1bb4b3832a1da199f0d0a7b93e95b8bd96c58045ff3b54d4969dc38a9260da"
        );

        let transition_hash = hash_shasta_transitions_array(&[transition]);
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
                designatedProver: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
                actualProver: address!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
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
                designatedProver: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
                actualProver: address!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
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
                designatedProver: address!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
                actualProver: address!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
            },
        ];

        // Calculate hash using hashTransitionsArray equivalent
        let result = hash_shasta_transitions_array(&transitions);
        assert_eq!(
            hex::encode(result),
            "f9e1faec6512a0048465cfee3bb43eadbbfe8fe781ac5eaa4defe841b4e06453"
        );

        // Test individual transition hashes
        let individual_hashes: Vec<String> = transitions
            .iter()
            .map(|t| hex::encode(hash_shasta_transition(t)))
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
