use alloy_sol_types::sol;
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BaseFeeConfig {
        uint8 adjustmentQuotient;
        uint8 sharingPctg;
        uint32 gasIssuancePerSecond;
        uint64 minGasExcess;
        uint32 maxGasIssuancePerBlock;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockParams {
        uint16 numTransactions;
        uint8 timeShift;
        bytes32[] signalSlots;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BatchInfo {
        bytes32 txsHash;
        BlockParams[] blocks;
        bytes32[] blobHashes;
        bytes32 extraData;
        address coinbase;
        uint64 proposedIn;
        uint64 blobCreatedIn;
        uint32 blobByteOffset;
        uint32 blobByteSize;
        uint32 gasLimit;
        uint64 lastBlockId;
        uint64 lastBlockTimestamp;
        uint64 anchorBlockId;
        bytes32 anchorBlockHash;
        BaseFeeConfig baseFeeConfig;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BatchMetadata {
        bytes32 infoHash;
        address proposer;
        uint64 batchId;
        uint64 proposedAt;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 stateRoot;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct TransitionState {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 stateRoot;
        address prover;
        bool inProvingWindow;
        uint48 createdAt;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Stats1 {
        uint64 genesisHeight;
        uint64 __reserved2;
        uint64 lastSyncedBatchId;
        uint64 lastSyncedAt;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Stats2 {
        uint64 numBatches;
        uint64 lastVerifiedBatchId;
        bool paused;
        uint56 lastProposedIn;
        uint64 lastUnpausedAt;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Config {
        uint64 chainId;
        uint64 maxUnverifiedBatches;
        uint64 batchRingBufferSize;
        uint64 maxBatchesToVerify;
        uint32 blockMaxGasLimit;
        uint96 livenessBondBase;
        uint96 livenessBondPerBlock;
        uint8 stateRootSyncInternal;
        uint64 maxAnchorHeightOffset;
        BaseFeeConfig baseFeeConfig;
        uint16 provingWindow;
        uint24 cooldownWindow;
        uint8 maxSignalsToReceive;
        uint16 maxBlocksPerBatch;
        ForkHeights forkHeights;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct ForkHeights {
        uint64 ontake;
        uint64 pacaya;
        uint64 shasta;
        uint64 unzen;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct Batch {
        bytes32 metaHash;
        uint64 lastBlockId;
        uint96 reserved3;
        uint96 livenessBond;
        uint64 batchId;
        uint64 lastBlockTimestamp;
        uint64 anchorBlockId;
        uint24 nextTransitionId;
        uint8 reserved4;
        uint24 verifiedTransitionId;
    }

    /// @notice Emitted when a batch is proposed.
    /// @param info The info of the proposed batch.
    /// @param meta The metadata of the proposed batch.
    /// @param txList The tx list in calldata.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BatchProposed(BatchInfo info, BatchMetadata meta, bytes txList);

    /// @notice Emitted when multiple transitions are proved.
    /// @param verifier The address of the verifier.
    /// @param batchIds The batch IDs.
    /// @param transitions The transitions data.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BatchesProved(address verifier, uint64[] batchIds, Transition[] transitions);

    /// @notice Emitted when a batch is verified.
    /// @param batchId The ID of the verified batch.
    /// @param blockHash The hash of the verified batch.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BatchesVerified(uint64 batchId, bytes32 blockHash);

    /// @notice Emitted when a token is credited back to a user's bond balance.
    /// @param user The address of the user whose bond balance is credited.
    /// @param amount The amount of tokens credited.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BondCredited(address indexed user, uint256 amount);

    /// @notice Emitted when a token is debited from a user's bond balance.
    /// @param user The address of the user whose bond balance is debited.
    /// @param amount The amount of tokens debited.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BondDebited(address indexed user, uint256 amount);

    /// @notice Emitted when tokens are deposited into a user's bond balance.
    /// @param user The address of the user who deposited the tokens.
    /// @param amount The amount of tokens deposited.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BondDeposited(address indexed user, uint256 amount);

    /// @notice Emitted when tokens are withdrawn from a user's bond balance.
    /// @param user The address of the user who withdrew the tokens.
    /// @param amount The amount of tokens withdrawn.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BondWithdrawn(address indexed user, uint256 amount);

    /// @notice Emitted when a transition is overwritten by a conflicting one.
    /// @param batchId The batch ID.
    /// @param oldTran The old transition overwritten.
    /// @param newTran The new transition.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event ConflictingProof(uint64 batchId, TransitionState oldTran, Transition newTran);

    /// @notice Emitted when a batch is synced.
    /// @param stats1 The Stats1 data structure.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event Stats1Updated(Stats1 stats1);

    /// @notice Emitted when some state variable values changed.
    /// @param stats2 The Stats2 data structure.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event Stats2Updated(Stats2 stats2);

    #[derive(Debug)]
    function proposeBatch(
        bytes calldata _params,
        bytes calldata _txList
    )
        external
        returns (BatchInfo memory info_, BatchMetadata memory meta_);

    function proveBatches(
        bytes calldata _params,
        bytes calldata _proof
    )
        external;

    function getBatch(uint64 _batchId)
        external
        view
        returns (Batch memory batch_);

    function getBatchVerifyingTransition(uint64 _batchId)
        external
        view
        returns (TransitionState memory);

    function getLastSyncedTransition()
        external
        view
        returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);

    function getLastVerifiedTransition()
        external
        view
        returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);

    function getStats1()
        external
        view
        returns (Stats1 memory);

    function getStats2()
        external
        view
        returns (Stats2 memory);

    function getTransitionById(uint64 _batchId, uint24 _tid)
        external
        view
        returns (TransitionState memory);

    function getTransitionByParentHash(uint64 _batchId, bytes32 _parentHash)
        external
        view
        returns (TransitionState memory);

    function pacayaConfig()
        external
        view
        returns (Config memory);

    function bondBalanceOf(address _user)
        external
        view
        returns (uint256);

    function bondToken()
        external
        view
        returns (address);

    function depositBond(uint256 _amount)
        external
        payable;

    function withdrawBond(uint256 _amount)
        external;
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
