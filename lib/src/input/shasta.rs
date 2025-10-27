use alloy_primitives::{Address, B256};
use alloy_sol_types::{sol, SolValue};
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @notice Represents a frame of data that is stored in multiple blobs. Note the size is
    /// encoded as a bytes32 at the offset location.
    struct BlobSlice {
        /// @notice The blobs containing the proposal's content.
        bytes32[] blobHashes;
        /// @notice The byte offset of the proposal's content in the containing blobs.
        uint24 offset;
        /// @notice The timestamp when the frame was created.
        uint48 timestamp;
    }

    #[derive(Debug, Deserialize, Serialize)]
    enum BondType {
        NONE,
        PROVABILITY,
        LIVENESS
    }

    #[derive(Debug, Deserialize, Serialize)]
    struct BondInstruction {
        uint48 proposalId;
        BondType bondType;
        address payer;
        address receiver;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Checkpoint {
        uint48 blockNumber;
        bytes32 blockHash;
        bytes32 stateRoot;
    }

    /// @notice Contains derivation data for a proposal that is not needed during proving.
    /// @dev This data is hashed and stored in the Proposal struct to reduce calldata size.
    #[derive(Debug, Default, Deserialize, Serialize)]

    /// @notice Represents a source of derivation data within a Derivation
    struct DerivationSource {
        /// @notice Whether this source is from a forced inclusion.
        bool isForcedInclusion;
        /// @notice Blobs that contain the source's manifest data.
        BlobSlice blobSlice;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @notice Contains derivation data for a proposal that is not needed during proving.
    /// @dev This data is hashed and stored in the Proposal struct to reduce calldata size.
    struct Derivation {
        /// @notice The L1 block number when the proposal was accepted.
        uint48 originBlockNumber;
        /// @notice The hash of the origin block.
        bytes32 originBlockHash;
        /// @notice The percentage of base fee paid to coinbase.
        uint8 basefeeSharingPctg;
        /// @notice Array of derivation sources, where each can be regular or forced inclusion.
        DerivationSource[] sources;
    }


    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @notice Represents a proposal for L2 blocks.
    struct Proposal {
        /// @notice Unique identifier for the proposal.
        uint48 id;
        /// @notice The L1 block timestamp when the proposal was accepted.
        uint48 timestamp;
        /// @notice The timestamp of the last slot where the current preconfer can propose.
        uint48 endOfSubmissionWindowTimestamp;
        /// @notice Address of the proposer.
        address proposer;
        /// @notice The current hash of coreState
        bytes32 coreStateHash;
        /// @notice Hash of the Derivation struct containing additional proposal data.
        bytes32 derivationHash;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Transition {
        bytes32 proposalHash;
        bytes32 parentTransitionHash;
        Checkpoint checkpoint;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @notice Represents the core state of the inbox.
    struct CoreState {
        /// @notice The next proposal ID to be assigned.
        uint48 nextProposalId;
        /// @notice The last block ID where a proposal was made.
        uint48 lastProposalBlockId;
        /// @notice The ID of the last finalized proposal.
        uint48 lastFinalizedProposalId;
        /// @notice The timestamp when the last checkpoint was saved.
        /// @dev In genesis block, this is set to 0 to allow the first checkpoint to be saved.
        uint48 lastCheckpointTimestamp;
        /// @notice The hash of the last finalized transition.
        bytes32 lastFinalizedTransitionHash;
        /// @notice The hash of all bond instructions.
        bytes32 bondInstructionsHash;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct ProposedEventPayload {
        Proposal proposal;
        Derivation derivation;
        CoreState coreState;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlobReference {
        uint16 blobStartIndex;
        uint16 numBlobs;
        uint24 offset;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    event Proposed(bytes data);

    #[derive(Debug, Default, Deserialize, Serialize)]
    event Proved(bytes data);

    #[derive(Debug, Default, Deserialize, Serialize)]
    event BondInstructed(BondInstruction[] instructions);
}

/// Decoded Shasta event data containing the proposal and related information
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ShastaEventData {
    pub proposal: Proposal,
    pub derivation: Derivation,
    pub core_state: CoreState,
}

impl ShastaEventData {
    /// Decode the bytes data from Shasta Proposed event into ShastaEventData
    pub fn from_event_data(data: &[u8]) -> Result<Self, alloy_sol_types::Error> {
        Self::decode_event_data(data)
    }

    fn _decode_event_data_with_abi(data: &[u8]) -> Result<Self, alloy_sol_types::Error> {
        let payload = ProposedEventPayload::abi_decode(data, true)?;
        Ok(Self {
            proposal: payload.proposal,
            derivation: payload.derivation,
            core_state: payload.coreState,
        })
    }

    fn unpack_uint24(data: &[u8], pos: usize) -> Result<(u32, usize), alloy_sol_types::Error> {
        // Ensure we have enough data for a 3-byte value
        if pos + 3 > data.len() {
            return Err(alloy_sol_types::Error::custom(
                "Not enough data to read 3-byte uint24".to_string(),
            ));
        }

        let value = u32::from_be_bytes([0, data[pos], data[pos + 1], data[pos + 2]]);
        // New position is old position + 3 bytes
        let new_pos = pos + 3;
        Ok((value, new_pos))
    }

    /// Unpacks a uint48 value from the data buffer at the given position
    /// Matches the Solidity mload behavior by reading a full 32-byte word and extracting 6 bytes
    fn unpack_uint48(data: &[u8], pos: usize) -> Result<(u64, usize), alloy_sol_types::Error> {
        // Ensure we have enough data for a full 32-byte word
        if pos + 6 > data.len() {
            return Err(alloy_sol_types::Error::custom(
                "Not enough data to read 32-byte word".to_string(),
            ));
        }

        let value = u64::from_be_bytes([
            0,
            0,
            data[pos + 0],
            data[pos + 1],
            data[pos + 2],
            data[pos + 3],
            data[pos + 4],
            data[pos + 5],
        ]);
        // New position is old position + 6 bytes
        let new_pos = pos + 6;
        Ok((value, new_pos))
    }

    fn unpack_address(data: &[u8], pos: usize) -> Result<(Address, usize), alloy_sol_types::Error> {
        if pos + 20 > data.len() {
            return Err(alloy_sol_types::Error::custom(
                "Not enough data to read 20-byte address".to_string(),
            ));
        }

        let address = Address::from_slice(&data[pos..pos + 20]);
        let new_pos = pos + 20;
        Ok((address, new_pos))
    }

    fn unpack_hash(data: &[u8], pos: usize) -> Result<(B256, usize), alloy_sol_types::Error> {
        if pos + 32 > data.len() {
            return Err(alloy_sol_types::Error::custom(
                "Not enough data to read 32-byte hash".to_string(),
            ));
        }

        let hash = B256::from_slice(&data[pos..pos + 32]);
        let new_pos = pos + 32;
        Ok((hash, new_pos))
    }

    /// Manual decoding of Shasta event data following the Solidity implementation
    /// taiko-mono://packages/protocol/contracts/layer1/shasta/libs/LibProposedEventEncoder.sol
    /// This provides an alternative to ABI decoding for cases where we need more control
    // fn decode_event_data(data: &[u8]) -> Result<Self, alloy_sol_types::Error> {
    //     // Decode Proposal
    //     let (proposal_id, ptr) = Self::unpack_uint48(data, 0)?;
    //     let (proposer, ptr) = Self::unpack_address(data, ptr)?;
    //     let (timestamp, ptr) = Self::unpack_uint48(data, ptr)?;
    //     let (end_of_submission_window_timestamp, ptr) = Self::unpack_uint48(data, ptr)?;

    //     // decode derivation
    //     let (origin_block_number, ptr) = Self::unpack_uint48(data, ptr)?;
    //     let (origin_block_hash, ptr) = Self::unpack_hash(data, ptr)?;

    //     let is_forced_inclusion = data[ptr] != 0;
    //     let ptr = ptr + 1;
    //     let basefee_sharing_pctg = data[ptr];
    //     let ptr = ptr + 1;

    //     let (blob_hashes_length, ptr) = Self::unpack_uint24(data, ptr)?;

    //     let mut blob_hashes = Vec::new();
    //     let mut ptr = ptr;
    //     for _ in 0..blob_hashes_length {
    //         let (blob_hash, new_ptr) = Self::unpack_hash(data, ptr)?;
    //         blob_hashes.push(blob_hash);
    //         ptr = new_ptr;
    //     }

    //     let (offset, ptr) = Self::unpack_uint24(data, ptr)?;
    //     let (blob_timestamp, ptr) = Self::unpack_uint48(data, ptr)?;
    //     let (core_state_hash, ptr) = Self::unpack_hash(data, ptr)?;
    //     let (derivation_hash, ptr) = Self::unpack_hash(data, ptr)?;

    //     // core state
    //     let (next_proposal_id, ptr) = Self::unpack_uint48(data, ptr)?;
    //     let (last_finalized_proposal_id, ptr) = Self::unpack_uint48(data, ptr)?;
    //     let (last_finalized_transition_hash, ptr) = Self::unpack_hash(data, ptr)?;
    //     let (bond_instructions_hash, _ptr) = Self::unpack_hash(data, ptr)?;

    //     Ok(Self {
    //         proposal: Proposal {
    //             id: proposal_id,
    //             timestamp,
    //             endOfSubmissionWindowTimestamp: end_of_submission_window_timestamp,
    //             proposer,
    //             coreStateHash: core_state_hash,
    //             derivationHash: derivation_hash,
    //         },
    //         derivation: Derivation {
    //             originBlockNumber: origin_block_number,
    //             originBlockHash: origin_block_hash,
    //             basefeeSharingPctg: basefee_sharing_pctg,
    //             sources: todo!(),
    //         },
    //         core_state: CoreState {
    //             nextProposalId: next_proposal_id,
    //             lastFinalizedProposalId: last_finalized_proposal_id,
    //             lastFinalizedTransitionHash: last_finalized_transition_hash,
    //             bondInstructionsHash: bond_instructions_hash,
    //         },
    //     })
    // }

    /// Add helper function to unpack uint16
    fn unpack_uint16(data: &[u8], pos: usize) -> Result<(u16, usize), alloy_sol_types::Error> {
        if pos + 2 > data.len() {
            return Err(alloy_sol_types::Error::custom(
                "Not enough data to read 2-byte uint16".to_string(),
            ));
        }

        let value = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let new_pos = pos + 2;
        Ok((value, new_pos))
    }

    /// Add helper function to unpack uint8
    fn unpack_uint8(data: &[u8], pos: usize) -> Result<(u8, usize), alloy_sol_types::Error> {
        if pos + 1 > data.len() {
            return Err(alloy_sol_types::Error::custom(
                "Not enough data to read 1-byte uint8".to_string(),
            ));
        }

        let value = data[pos];
        let new_pos = pos + 1;
        Ok((value, new_pos))
    }

    /// Manual decoding of Shasta event data following the Solidity implementation
    /// Reference: taiko-mono/packages/protocol/contracts/layer1/shasta/libs/LibProposedEventEncoder.sol
    fn decode_event_data(data: &[u8]) -> Result<Self, alloy_sol_types::Error> {
        let mut ptr = 0;

        // Decode Proposal
        let (proposal_id, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;
        let (proposer, new_ptr) = Self::unpack_address(data, ptr)?;
        ptr = new_ptr;
        let (timestamp, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;
        let (end_of_submission_window_timestamp, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;

        // Decode derivation fields
        let (origin_block_number, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;
        let (origin_block_hash, new_ptr) = Self::unpack_hash(data, ptr)?;
        ptr = new_ptr;
        let (basefee_sharing_pctg, new_ptr) = Self::unpack_uint8(data, ptr)?;
        ptr = new_ptr;

        // Decode sources array length
        let (sources_length, new_ptr) = Self::unpack_uint16(data, ptr)?;
        ptr = new_ptr;

        let mut sources = Vec::new();
        for _ in 0..sources_length {
            // Decode is_forced_inclusion flag
            let (is_forced_inclusion_u8, new_ptr) = Self::unpack_uint8(data, ptr)?;
            ptr = new_ptr;
            let is_forced_inclusion = is_forced_inclusion_u8 != 0;

            // Decode blob slice for this source
            let (blob_hashes_length, new_ptr) = Self::unpack_uint16(data, ptr)?;
            ptr = new_ptr;

            let mut blob_hashes = Vec::new();
            for _ in 0..blob_hashes_length {
                let (blob_hash, new_ptr) = Self::unpack_hash(data, ptr)?;
                ptr = new_ptr;
                blob_hashes.push(blob_hash);
            }

            let (offset, new_ptr) = Self::unpack_uint24(data, ptr)?;
            ptr = new_ptr;
            let (blob_timestamp, new_ptr) = Self::unpack_uint48(data, ptr)?;
            ptr = new_ptr;

            sources.push(DerivationSource {
                isForcedInclusion: is_forced_inclusion,
                blobSlice: BlobSlice {
                    blobHashes: blob_hashes,
                    offset,
                    timestamp: blob_timestamp,
                },
            });
        }

        let (core_state_hash, new_ptr) = Self::unpack_hash(data, ptr)?;
        ptr = new_ptr;
        let (derivation_hash, new_ptr) = Self::unpack_hash(data, ptr)?;
        ptr = new_ptr;

        // Decode core state
        let (next_proposal_id, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;
        let (last_proposal_block_id, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;
        let (last_finalized_proposal_id, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;
        let (last_checkpoint_timestamp, new_ptr) = Self::unpack_uint48(data, ptr)?;
        ptr = new_ptr;
        let (last_finalized_transition_hash, new_ptr) = Self::unpack_hash(data, ptr)?;
        ptr = new_ptr;
        let (bond_instructions_hash, _new_ptr) = Self::unpack_hash(data, ptr)?;

        Ok(Self {
            proposal: Proposal {
                id: proposal_id,
                timestamp,
                endOfSubmissionWindowTimestamp: end_of_submission_window_timestamp,
                proposer,
                coreStateHash: core_state_hash,
                derivationHash: derivation_hash,
            },
            derivation: Derivation {
                originBlockNumber: origin_block_number,
                originBlockHash: origin_block_hash,
                basefeeSharingPctg: basefee_sharing_pctg,
                sources,
            },
            core_state: CoreState {
                nextProposalId: next_proposal_id,
                lastProposalBlockId: last_proposal_block_id,
                lastFinalizedProposalId: last_finalized_proposal_id,
                lastCheckpointTimestamp: last_checkpoint_timestamp,
                lastFinalizedTransitionHash: last_finalized_transition_hash,
                bondInstructionsHash: bond_instructions_hash,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use reth_primitives::{address, b256};

    use crate::input::{shasta::ShastaEventData, GuestInput};

    #[test]
    fn input_serde_roundtrip() {
        let input = GuestInput::default();
        let _: GuestInput = bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }

    #[test]
    fn test_manual_decode_shasta_event_data() {
        // Test the manual decoding function to ensure it works correctly
        // Using the same data as the ABI decoding test
        let data = hex::decode("00000000026f3c44cdddb6a900fa2b585dd299e03d12fa4293bc000068fef1e40000000000000000000012b1c8d4d43b58fb5af9d21af9f575349274cae13fb42a39577d9a097b3685b9f0d24b00010000010162610b05a7d5a71bc7b6621cdd9bafbfbdf24dfc825210f1f1c68496e8e569000000000068fef1e4b58ff663a85896e6d5389e25fa5cbc8db864266a4e0511829a671111a50c9bd4936490fe7bdf8fd6185cddd3e8d36b9c2c15e06cf9a1f0c99a3a9966a1e8ed8d0000000002700000000012b2000000000263000068fef1e491dab1dbe9ea94a0b4b325f30c34742edde00b8dea6d04a2f2e6a748eb35ac330000000000000000000000000000000000000000000000000000000000000000").unwrap();

        // Decode using manual decoding function
        let result = ShastaEventData::decode_event_data(&data);

        assert!(
            result.is_ok(),
            "Failed to manually decode Shasta event data: {:?}",
            result.err()
        );

        let event_data = result.unwrap();

        // Assert proposal fields
        assert_eq!(event_data.proposal.id, 623);
        assert_eq!(
            event_data.proposal.proposer,
            address!("3C44CdDdB6a900fa2b585dd299e03d12FA4293BC")
        );
        assert_eq!(event_data.proposal.timestamp, 1761538532);
        assert_eq!(event_data.proposal.endOfSubmissionWindowTimestamp, 0);
        assert_eq!(
            event_data.proposal.coreStateHash,
            b256!("b58ff663a85896e6d5389e25fa5cbc8db864266a4e0511829a671111a50c9bd4")
        );
        assert_eq!(
            event_data.proposal.derivationHash,
            b256!("936490fe7bdf8fd6185cddd3e8d36b9c2c15e06cf9a1f0c99a3a9966a1e8ed8d")
        );

        // Assert derivation fields
        assert_eq!(event_data.derivation.originBlockNumber, 4785);
        assert_eq!(
            event_data.derivation.originBlockHash,
            b256!("c8d4d43b58fb5af9d21af9f575349274cae13fb42a39577d9a097b3685b9f0d2")
        );
        assert_eq!(event_data.derivation.basefeeSharingPctg, 75);

        // Assert blob slice fields
        assert_eq!(event_data.derivation.sources.len(), 1);
        assert_eq!(
            event_data.derivation.sources[0].blobSlice.blobHashes[0],
            b256!("0162610b05a7d5a71bc7b6621cdd9bafbfbdf24dfc825210f1f1c68496e8e569")
        );
        assert_eq!(event_data.derivation.sources[0].blobSlice.offset, 0);
        assert_eq!(
            event_data.derivation.sources[0].blobSlice.timestamp,
            1761538532
        );

        // Assert core state fields
        assert_eq!(event_data.core_state.nextProposalId, 624);
        assert_eq!(event_data.core_state.lastProposalBlockId, 4786);
        assert_eq!(event_data.core_state.lastFinalizedProposalId, 611);
        assert_eq!(
            event_data.core_state.lastFinalizedTransitionHash,
            b256!("91dab1dbe9ea94a0b4b325f30c34742edde00b8dea6d04a2f2e6a748eb35ac33")
        );
        assert_eq!(
            event_data.core_state.bondInstructionsHash,
            b256!("0000000000000000000000000000000000000000000000000000000000000000")
        );
    }
}
