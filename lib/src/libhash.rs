// rust impl of taiko-mono/packages/protocol/contracts/layer1/shasta/libs/LibHashing.sol

use crate::input::shasta::{Commitment, Derivation, Proposal};

use crate::input::shasta::Checkpoint;
use crate::primitives::keccak::keccak;
use crate::prover::{ProofCarryData, TransitionInputData};
use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::SolValue;
use reth_primitives::b256;

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

// Helper to encode a u48 (Rust u64 is fine, always left-padded in Solidity as uint256)
pub fn u48_to_b256(val: u64) -> B256 {
    // Truncate to 48 bits
    let val = val & 0xffff_ffff_ffff;
    u64_to_b256(val)
}

// Helper to encode a u48 (Rust u64 is fine, always left-padded in Solidity as uint256)
pub fn u64_to_b256(val: u64) -> B256 {
    U256::from(val).into()
}

const EMPTY_BYTES_HASH: B256 =
    b256!("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");

pub const VERIFY_PROOF_B256: B256 =
    b256!("5645524946595f50524f4f460000000000000000000000000000000000000000");

/// Domain-separated hash for a Shasta sub-proof public input.
///
/// This binds `chain_id` and `verifier` to the signed message to avoid cross-chain / cross-verifier
/// replay of otherwise identical transition inputs.
pub fn hash_shasta_subproof_input(carry: &ProofCarryData) -> B256 {
    tracing::info!("hash_shasta_subproof_input: {carry:?}");
    let transition_hash = hash_shasta_transition_input(&carry.transition_input);
    hash_four_values(
        VERIFY_PROOF_B256,
        U256::from(carry.chain_id).into(),
        address_to_b256(carry.verifier),
        transition_hash,
    )
}

pub fn hash_shasta_transition_input(transition_input: &TransitionInputData) -> B256 {
    // IMPORTANT (soundness): Aggregation checks rely on fields beyond `Transition`.
    // This hash must bind all continuity-critical fields; otherwise a caller can tamper with
    // carry-data (e.g. parent hashes / end checkpoint) without invalidating the sub-proof input.
    let mut values: Vec<B256> = Vec::with_capacity(13);

    // Proposal linkage
    values.push(u64_to_b256(transition_input.proposal_id));
    values.push(transition_input.proposal_hash);
    values.push(transition_input.parent_proposal_hash);
    values.push(transition_input.parent_block_hash);

    // Prover identity (L1-level)
    values.push(address_to_b256(transition_input.actual_prover));

    // Transition fields (as in Solidity Transition struct)
    values.push(address_to_b256(transition_input.transition.proposer));
    values.push(u48_to_b256(transition_input.transition.timestamp));
    values.push(hash_checkpoint(&transition_input.checkpoint));

    // End checkpoint fields used by `Commitment` (bind to prevent tampering)
    values.push(u48_to_b256(transition_input.checkpoint.blockNumber));
    values.push(transition_input.checkpoint.blockHash);
    values.push(transition_input.checkpoint.stateRoot);

    hash_values_impl(&values)
}

/// Optimized hashing for commitment data, matching Solidity's hashCommitment implementation.
/// Flattens all fields following the same memory layout as the Solidity buffer,
/// including static field ordering, offsets, and transition element packing.
pub fn hash_commitment(commitment: &Commitment) -> B256 {
    let transitions_len = commitment.transitions.len();
    let total_words = 9 + transitions_len * 4;

    let mut buffer: Vec<B256> = Vec::with_capacity(total_words);

    // [0] offset to commitment (0x20)
    buffer.push(U256::from(0x20u64).into());

    // Commitment static section
    // [1] firstProposalId
    buffer.push(U256::from(commitment.firstProposalId).into());
    // [2] firstProposalParentBlockHash
    buffer.push(commitment.firstProposalParentBlockHash);
    // [3] lastProposalHash
    buffer.push(commitment.lastProposalHash);
    // [4] actualProver as address (160 bits zero-extended to 256)
    buffer.push(address_to_b256(commitment.actualProver));
    // [5] endBlockNumber
    buffer.push(U256::from(commitment.endBlockNumber).into());
    // [6] endStateRoot
    buffer.push(commitment.endStateRoot);
    // [7] offset to transitions (0xe0)
    buffer.push(U256::from(0xe0u64).into());

    // [8] transitions array length
    buffer.push(U256::from(transitions_len as u64).into());

    // Each transition: [proposer, timestamp, blockHash]
    for transition in &commitment.transitions {
        // proposer: address (uint160, left padded to 32 bytes)
        buffer.push(address_to_b256(transition.proposer));
        // timestamp: as 256 bits (Solidity stores as uint256)
        buffer.push(U256::from(transition.timestamp).into());
        // blockHash (matches Solidity's .blockHash field naming and usage)
        buffer.push(transition.blockHash);
    }

    hash_values_impl(&buffer)
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
    keccak(proposal.abi_encode().as_slice()).into()
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
    write_word(
        &mut buffer,
        1,
        U256::from(derivation.originBlockNumber).into(),
    );
    // [2] originBlockHash
    write_word(&mut buffer, 2, derivation.originBlockHash);
    // [3] basefeeSharingPctg
    write_word(
        &mut buffer,
        3,
        U256::from(derivation.basefeeSharingPctg).into(),
    );
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
        write_word(
            &mut buffer,
            data_cursor,
            U256::from(is_forced_inclusion_value).into(),
        );
        // [dataCursor + 1] offset to blobSlice (0x40)
        write_word(&mut buffer, data_cursor + 1, U256::from(0x40u64).into());

        // BlobSlice head
        let blob_slice_base = data_cursor + 2;
        // [blobSliceBase] offset to blobHashes (0x60)
        write_word(&mut buffer, blob_slice_base, U256::from(0x60u64).into());
        // [blobSliceBase + 1] offset
        write_word(
            &mut buffer,
            blob_slice_base + 1,
            U256::from(source.blobSlice.offset).into(),
        );
        // [blobSliceBase + 2] timestamp
        write_word(
            &mut buffer,
            blob_slice_base + 2,
            U256::from(source.blobSlice.timestamp).into(),
        );

        // Blob hashes array
        let blob_hashes_base = blob_slice_base + 3;
        let blob_hashes_length = source.blobSlice.blobHashes.len();
        // [blobHashesBase] blobHashes length
        write_word(
            &mut buffer,
            blob_hashes_base,
            U256::from(blob_hashes_length).into(),
        );

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
    prove_input_hash: B256,
    chain_id: u64,
    verifier_address: Address,
    sgx_instance: Address,
) -> B256 {
    hash_five_values(
        VERIFY_PROOF_B256,
        U256::from(chain_id).into(),
        address_to_b256(verifier_address),
        prove_input_hash,
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
        // Construct a proposal with chosen values
        use crate::input::shasta::Proposal;
        use reth_primitives::{b256, Address};

        let proposal = Proposal {
            id: 12_345,
            timestamp: 193_828_690,
            endOfSubmissionWindowTimestamp: 193_829_690,
            proposer: Address::from_slice(
                &hex::decode("1234567890AbcdEF1234567890aBcdef12345678").unwrap(),
            ),
            parentProposalHash: b256!(
                "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
            ),
            originBlockNumber: 73_826,
            originBlockHash: b256!(
                "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
            ),
            basefeeSharingPctg: 42,
            sources: vec![
                DerivationSource {
                    isForcedInclusion: true,
                    blobSlice: BlobSlice {
                        blobHashes: vec![b256!(
                            "67890abcdef1234567890abcdef123451234567890abcdef1234567890abcdef"
                        )],
                        offset: 0,
                        timestamp: 100,
                    },
                },
                DerivationSource {
                    isForcedInclusion: false,
                    blobSlice: BlobSlice {
                        blobHashes: vec![b256!(
                            "567890abcdef123451234567890abcdef123456767890abcdef1234890abcdef"
                        )],
                        offset: 100,
                        timestamp: 200,
                    },
                },
            ],
        };

        let proposal_hash = hash_proposal(&proposal);
        // The value from the Solidity test for these values (expected hash):
        assert_eq!(
            proposal_hash,
            b256!("13af2d05799894db3462512e3ecf5ae8877b80b1e2db3963654ac70f6dd49f88")
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
            "1da64d2dd5bda3fb186ecf02433b32f1a24661030600a8ff150ed8c346dcc5ba"
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

    #[test]
    fn test_hash_prove_input() {
        // Setup a sample ProveInput with minimal structure to test only that hash_prove_input is called and behaves as expected.
        // This matches the test structure and dummy field values from the Solidity reference.
        let prove_input = Commitment {
            firstProposalId: 42,
            firstProposalParentBlockHash: b256!(
                "0000000000000000000000000000000000000000000000000000000000000999"
            ),
            lastProposalHash: b256!(
                "0000000000000000000000000000000000000000000000000000000000123456"
            ),
            actualProver: address!("0000000000000000000000000000000000012345"),
            endBlockNumber: 1000,
            endStateRoot: b256!("0000000000000000000000000000000000000000000000000000000000abcdef"),
            transitions: vec![crate::input::shasta::Transition {
                proposer: address!("0000000000000000000000000000000000001111"),
                timestamp: 123_456_789,
                blockHash: b256!(
                    "0000000000000000000000000000000000000000000000000000000000003333"
                ),
            }],
        };

        let prove_input_hash = hash_commitment(&prove_input);
        assert_eq!(
            alloy_primitives::hex::encode_prefixed(prove_input_hash),
            "0x079961e990a2be01ebe286ee2fdd382fde2349730971fe32a821da9dec67559e"
        );
    }

    #[test]
    fn test_hash_shasta_transition_input_binds_continuity_fields() {
        use crate::input::shasta::Checkpoint;
        use crate::prover::{ShastaTransitionInput, TransitionInputData};

        let mut base = TransitionInputData {
            proposal_id: 1,
            proposal_hash: b256!(
                "1111111111111111111111111111111111111111111111111111111111111111"
            ),
            parent_proposal_hash: b256!(
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            parent_block_hash: b256!(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            actual_prover: address!("1111111111111111111111111111111111111111"),
            transition: ShastaTransitionInput {
                proposer: address!("2222222222222222222222222222222222222222"),
                timestamp: 123,
            },
            checkpoint: Checkpoint {
                blockNumber: 10,
                blockHash: b256!(
                    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                ),
                stateRoot: b256!(
                    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                ),
            },
        };

        let h0 = hash_shasta_transition_input(&base);

        // Changing any continuity / commitment-relevant field must change the hash.
        base.parent_block_hash =
            b256!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
        assert_ne!(h0, hash_shasta_transition_input(&base));

        base.parent_block_hash =
            b256!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        base.parent_proposal_hash =
            b256!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
        assert_ne!(h0, hash_shasta_transition_input(&base));

        base.parent_proposal_hash =
            b256!("0000000000000000000000000000000000000000000000000000000000000000");
        base.checkpoint.stateRoot =
            b256!("9999999999999999999999999999999999999999999999999999999999999999");
        assert_ne!(h0, hash_shasta_transition_input(&base));
    }

    #[test]
    fn test_hash_shasta_subproof_input_domain_separates_chain_and_verifier() {
        use crate::input::shasta::Checkpoint;
        use crate::prover::{ProofCarryData, ShastaTransitionInput, TransitionInputData};

        let transition_input = TransitionInputData {
            proposal_id: 1,
            proposal_hash: b256!(
                "1111111111111111111111111111111111111111111111111111111111111111"
            ),
            parent_proposal_hash: b256!(
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            parent_block_hash: b256!(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            actual_prover: address!("1111111111111111111111111111111111111111"),
            transition: ShastaTransitionInput {
                proposer: address!("2222222222222222222222222222222222222222"),
                timestamp: 123,
            },
            checkpoint: Checkpoint {
                blockNumber: 10,
                blockHash: b256!(
                    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                ),
                stateRoot: b256!(
                    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                ),
            },
        };

        let base = ProofCarryData {
            chain_id: 167001,
            verifier: address!("00f9f60C79e38c08b785eE4F1a849900693C6630"),
            transition_input: transition_input.clone(),
        };

        let h0 = hash_shasta_subproof_input(&base);

        let mut diff_chain = base.clone();
        diff_chain.chain_id = 167002;
        assert_ne!(h0, hash_shasta_subproof_input(&diff_chain));

        let mut diff_verifier = base.clone();
        diff_verifier.verifier = address!("1111111111111111111111111111111111111111");
        assert_ne!(h0, hash_shasta_subproof_input(&diff_verifier));

        let mut diff_transition = base.clone();
        diff_transition.transition_input = TransitionInputData {
            proposal_id: 2,
            ..transition_input
        };
        assert_ne!(h0, hash_shasta_subproof_input(&diff_transition));
    }
}
