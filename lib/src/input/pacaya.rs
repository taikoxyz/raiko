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
        // Signals sent on L1 and need to sync to this L2 block.
        bytes32[] signalSlots;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlobParams {
        // The hashes of the blob. Note that if this array is not empty.  `firstBlobIndex` and
        // `numBlobs` must be 0.
        bytes32[] blobHashes;
        // The index of the first blob in this batch.
        uint8 firstBlobIndex;
        // The number of blobs in this batch. Blobs are initially concatenated and subsequently
        // decompressed via Zlib.
        uint8 numBlobs;
        // The byte offset of the blob in the batch.
        uint32 byteOffset;
        // The byte size of the blob.
        uint32 byteSize;
        // The block number when the blob was created.
        uint64 createdIn;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BatchParams {
        address proposer;
        address coinbase;
        bytes32 parentMetaHash;
        uint64 anchorBlockId;
        bool revertIfNotFirstProposal;
        // Specifies the number of blocks to be generated from this batch.
        BlobParams blobParams;
        BlockParams[] blocks;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @dev This struct holds batch information essential for constructing blocks offchain, but it
    /// does not include data necessary for batch proving.
    struct BatchInfo {
        bytes32 txsHash;
        // Data to build L2 blocks
        BlockParams[] blocks;
        bytes32[] blobHashes;
        bytes32 extraData;
        address coinbase;
        uint64 proposedIn; // Used by node/client
        uint64 blobCreatedIn;
        uint32 blobByteOffset;
        uint32 blobByteSize;
        uint32 gasLimit;
        uint64 lastBlockId;
        // Data for the L2 anchor transaction, shared by all blocks in the batch
        uint64 anchorBlockId;
        // corresponds to the `_anchorStateRoot` parameter in the anchor transaction.
        // The batch's validity proof shall verify the integrity of these two values.
        bytes32 anchorBlockHash;
        Config config;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @dev This struct holds batch metadata essential for proving the batch.
    struct BatchMetadata {
        bytes32 infoHash;
        address proposer;
        uint64 batchId;
        uint64 proposedAt; // Used by node/client
    }


    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @notice Struct representing transition to be proven.
    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 stateRoot;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct ForkHeights {
        uint64 ontake;
        uint64 pacaya;
        uint64 shasta;
        uint64 unzen;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    /// @notice Struct holding Taiko configuration parameters. See {TaikoConfig}.
    struct Config {
        /// @notice The chain ID of the network where Taiko contracts are deployed.
        uint64 chainId;
        /// @notice The maximum number of unverified batches the protocol supports.
        uint64 maxUnverifiedBatches;
        /// @notice Size of the batch ring buffer, allowing extra space for proposals.
        uint64 batchRingBufferSize;
        /// @notice The maximum number of verifications allowed when a batch is proposed or proved.
        uint64 maxBatchesToVerify;
        /// @notice The maximum gas limit allowed for a block.
        uint32 blockMaxGasLimit;
        /// @notice The amount of Taiko token as a prover liveness bond per batch.
        uint96 livenessBondBase;
        /// @notice The amount of Taiko token as a prover liveness bond per block.
        uint96 livenessBondPerBlock;
        /// @notice The number of batches between two L2-to-L1 state root sync.
        uint8 stateRootSyncInternal;
        /// @notice The max differences of the anchor height and the current block number.
        uint64 maxAnchorHeightOffset;
        /// @notice Base fee configuration
        BaseFeeConfig baseFeeConfig;
        /// @notice The proving window in seconds.
        uint16 provingWindow;
        /// @notice The time required for a transition to be used for verifying a batch.
        uint24 cooldownWindow;
        /// @notice The maximum number of signals to be received by TaikoL2.
        uint8 maxSignalsToReceive;
        /// @notice The maximum number of blocks per batch.
        uint16 maxBlocksPerBatch;
        /// @notice Historical heights of the forks.
        ForkHeights forkHeights;
    }

    /// @notice Emitted when a batch is proposed.
    /// @param info The info of the proposed batch.
    /// @param meta The metadata of the proposed batch.
    /// @param txList The tx list in calldata.
    #[derive(Debug, Default, Deserialize, Serialize)]
    event BatchProposed(BatchInfo info, BatchMetadata meta, bytes txList);

    #[derive(Debug)]
    /// @notice Proposes a batch of blocks.
    /// @param _params ABI-encoded BlockParams.
    /// @param _txList The transaction list in calldata. If the txList is empty, blob will be used
    /// for data availability.
    /// @return info_ The info of the proposed batch.
    /// @return meta_ The metadata of the proposed batch.
    function proposeBatch(
        bytes calldata _params,
        bytes calldata _txList
    )
        external
        returns (BatchInfo memory info_, BatchMetadata memory meta_);

    /// @notice Proves state transitions for multiple batches with a single aggregated proof.
    /// @param _params ABI-encoded parameter containing:
    /// - metas: Array of metadata for each batch being proved.
    /// - transitions: Array of batch transitions to be proved.
    /// @param _proof The aggregated cryptographic proof proving the batches transitions.
    function proveBatches(bytes calldata _params, bytes calldata _proof) external;
}
