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
        uint64 blockNumber;
        bytes32 blockHash;
        bytes32 stateRoot;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Config {
        address bondToken;
        uint48 provingWindow;
        uint48 extendedProvingWindow;
        uint256 maxFinalizationCount;
        uint256 ringBufferSize;
        uint8 basefeeSharingPctg;
        address checkpointManager;
        address proofVerifier;
        address proposerChecker;
        uint256 minForcedInclusionCount;
        uint64 forcedInclusionDelay;
        uint64 forcedInclusionFeeInGwei;
    }

    /// @notice Contains derivation data for a proposal that is not needed during proving.
    /// @dev This data is hashed and stored in the Proposal struct to reduce calldata size.
    #[derive(Debug, Default, Deserialize, Serialize)]

    struct Derivation {
        /// @notice The L1 block number when the proposal was accepted.
        uint48 originBlockNumber;
        /// @notice The hash of the origin block.
        bytes32 originBlockHash;
        /// @notice Whether the proposal is from a forced inclusion.
        bool isForcedInclusion;
        /// @notice The percentage of base fee paid to coinbase.
        uint8 basefeeSharingPctg;
        /// @notice Blobs that contains the proposal's manifest data.
        BlobSlice blobSlice;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
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
        address designatedProver;
        address actualProver;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct TransitionRecord {
        uint8 span;
        BondInstruction[] bondInstructions;
        bytes32 transitionHash;
        bytes32 checkpointHash;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @notice Represents the core state of the inbox.
    struct CoreState {
        /// @notice The next proposal ID to be assigned.
        uint48 nextProposalId;
        /// @notice The ID of the last finalized proposal.
        uint48 lastFinalizedProposalId;
        /// @notice The hash of the last finalized transition.
        bytes32 lastFinalizedTransitionHash;
        /// @notice The hash of all bond instructions.
        bytes32 bondInstructionsHash;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct ProposeInput {
        uint48 deadline;
        CoreState coreState;
        Proposal[] parentProposals;
        BlobReference blobReference;
        TransitionRecord[] transitionRecords;
        Checkpoint checkpoint;
        uint8 numForcedInclusions;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct ProveInput {
        Proposal[] proposals;
        Transition[] transitions;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct ProposedEventPayload {
        Proposal proposal;
        Derivation derivation;
        CoreState coreState;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct ProvedEventPayload {
        uint48 proposalId;
        Transition transition;
        TransitionRecord transitionRecord;
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
    fn decode_event_data(data: &[u8]) -> Result<Self, alloy_sol_types::Error> {
        // Decode Proposal
        let (proposal_id, ptr) = Self::unpack_uint48(data, 0)?;
        let (proposer, ptr) = Self::unpack_address(data, ptr)?;
        let (timestamp, ptr) = Self::unpack_uint48(data, ptr)?;
        let (end_of_submission_window_timestamp, ptr) = Self::unpack_uint48(data, ptr)?;

        // decode derivation
        let (origin_block_number, ptr) = Self::unpack_uint48(data, ptr)?;
        let (origin_block_hash, ptr) = Self::unpack_hash(data, ptr)?;

        let is_forced_inclusion = data[ptr] != 0;
        let ptr = ptr + 1;
        let basefee_sharing_pctg = data[ptr];
        let ptr = ptr + 1;

        let (blob_hashes_length, ptr) = Self::unpack_uint24(data, ptr)?;

        let mut blob_hashes = Vec::new();
        let mut ptr = ptr;
        for _ in 0..blob_hashes_length {
            let (blob_hash, new_ptr) = Self::unpack_hash(data, ptr)?;
            blob_hashes.push(blob_hash);
            ptr = new_ptr;
        }

        let (offset, ptr) = Self::unpack_uint24(data, ptr)?;
        let (blob_timestamp, ptr) = Self::unpack_uint48(data, ptr)?;
        let (core_state_hash, ptr) = Self::unpack_hash(data, ptr)?;
        let (derivation_hash, ptr) = Self::unpack_hash(data, ptr)?;

        // core state
        let (next_proposal_id, ptr) = Self::unpack_uint48(data, ptr)?;
        let (last_finalized_proposal_id, ptr) = Self::unpack_uint48(data, ptr)?;
        let (last_finalized_transition_hash, ptr) = Self::unpack_hash(data, ptr)?;
        let (bond_instructions_hash, _ptr) = Self::unpack_hash(data, ptr)?;

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
                isForcedInclusion: is_forced_inclusion,
                basefeeSharingPctg: basefee_sharing_pctg,
                blobSlice: BlobSlice {
                    blobHashes: blob_hashes,
                    offset,
                    timestamp: blob_timestamp,
                },
            },
            core_state: CoreState {
                nextProposalId: next_proposal_id,
                lastFinalizedProposalId: last_finalized_proposal_id,
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
        let data = hex::decode("0000000000893c44cdddb6a900fa2b585dd299e03d12fa4293bc000068c3c67000000000000000000000040721cc61cab68d0d851739c9e224c2ae25f92be44179b456f2eaf89a47f749b640004b00000101bdb66a0c55182e0d9cece2ee7c7adbd88e7f6dbfb69797310b3f12f971e6c5000000000068c3c6706171761baaf8082dfc92628ab484a890271acccf75bae15f375829ca281939db8e68ea9bc41998e2770c72a172b0fc2bec880cba7471723d773872ec061ed5b600000000008a000000000000af85b1090dba108cef8fdffbb2931222ddf7e5c19520be0480a49f3728d01da80000000000000000000000000000000000000000000000000000000000000000").unwrap();

        // Decode using manual decoding function
        let result = ShastaEventData::decode_event_data(&data);

        assert!(
            result.is_ok(),
            "Failed to manually decode Shasta event data: {:?}",
            result.err()
        );

        let event_data = result.unwrap();

        // Assert proposal fields
        assert_eq!(event_data.proposal.id, 137);
        assert_eq!(
            event_data.proposal.proposer,
            address!("3C44CdDdB6a900fa2b585dd299e03d12FA4293BC")
        );
        assert_eq!(event_data.proposal.timestamp, 1757660784);
        assert_eq!(event_data.proposal.endOfSubmissionWindowTimestamp, 0);
        assert_eq!(
            event_data.proposal.coreStateHash,
            b256!("6171761baaf8082dfc92628ab484a890271acccf75bae15f375829ca281939db")
        );
        assert_eq!(
            event_data.proposal.derivationHash,
            b256!("8e68ea9bc41998e2770c72a172b0fc2bec880cba7471723d773872ec061ed5b6")
        );

        // Assert derivation fields
        assert_eq!(event_data.derivation.originBlockNumber, 1031);
        assert_eq!(
            event_data.derivation.originBlockHash,
            b256!("21cc61cab68d0d851739c9e224c2ae25f92be44179b456f2eaf89a47f749b640")
        );
        assert_eq!(event_data.derivation.isForcedInclusion, false);
        assert_eq!(event_data.derivation.basefeeSharingPctg, 75);

        // Assert blob slice fields
        assert_eq!(event_data.derivation.blobSlice.blobHashes.len(), 1);
        assert_eq!(
            event_data.derivation.blobSlice.blobHashes[0],
            b256!("01bdb66a0c55182e0d9cece2ee7c7adbd88e7f6dbfb69797310b3f12f971e6c5")
        );
        assert_eq!(event_data.derivation.blobSlice.offset, 0);
        assert_eq!(event_data.derivation.blobSlice.timestamp, 1757660784);

        // Assert core state fields
        assert_eq!(event_data.core_state.nextProposalId, 138);
        assert_eq!(event_data.core_state.lastFinalizedProposalId, 0);
        assert_eq!(
            event_data.core_state.lastFinalizedTransitionHash,
            b256!("af85b1090dba108cef8fdffbb2931222ddf7e5c19520be0480a49f3728d01da8")
        );
        assert_eq!(
            event_data.core_state.bondInstructionsHash,
            b256!("0000000000000000000000000000000000000000000000000000000000000000")
        );
    }
}
