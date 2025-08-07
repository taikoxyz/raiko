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
    struct BlockParamsV2 {
        address coinbase;
        bytes32 parentMetaHash;
        uint64 anchorBlockId; // NEW
        uint64 timestamp; // NEW
        uint32 blobTxListOffset; // NEW
        uint32 blobTxListLength; // NEW
        uint8 blobIndex; // NEW
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockMetadataV2 {
        bytes32 anchorBlockHash; // `_l1BlockHash` in TaikoL2's anchor tx.
        bytes32 difficulty;
        bytes32 blobHash;
        bytes32 extraData;
        address coinbase;
        uint64 id;
        uint32 gasLimit;
        uint64 timestamp;
        uint64 anchorBlockId; // `_l1BlockId` in TaikoL2's anchor tx.
        uint16 minTier;
        bool blobUsed;
        bytes32 parentMetaHash;
        address proposer;
        uint96 livenessBond;
        // Time this block is proposed at, used to check proving window and cooldown window.
        uint64 proposedAt;
        // L1 block number, required/used by node/client.
        uint64 proposedIn;
        uint32 blobTxListOffset;
        uint32 blobTxListLength;
        uint8 blobIndex;
        BaseFeeConfig baseFeeConfig;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    event BlockProposedV2(uint256 indexed blockId, BlockMetadataV2 meta);

    #[derive(Debug, Default, Deserialize, Serialize)]
    event CalldataTxList(uint256 indexed blockId, bytes txList);

    #[derive(Debug)]
    function proposeBlockV2(
        bytes calldata _params,
        bytes calldata _txList
    )
    {}

    function proveBlock(uint64 blockId, bytes calldata input) {}
}
