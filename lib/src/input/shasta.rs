use alloy_sol_types::sol;
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlobSlice {
        bytes32[] blobHashes;
        uint24 offset;
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

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Derivation {
        uint48 originBlockNumber;
        bytes32 originBlockHash;
        bool isForcedInclusion;
        uint8 basefeeSharingPctg;
        BlobSlice blobSlice;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Proposal {
        uint48 id;
        address proposer;
        uint48 timestamp;
        bytes32 coreStateHash;
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
    struct CoreState {
        uint48 nextProposalId;
        uint48 lastFinalizedProposalId;
        bytes32 lastFinalizedTransitionHash;
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

#[cfg(test)]
mod tests {
    extern crate alloc;
    use crate::input::GuestInput;

    #[test]
    fn input_serde_roundtrip() {
        let input = GuestInput::default();
        let _: GuestInput = bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }
}
