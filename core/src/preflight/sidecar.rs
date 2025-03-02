use raiko_lib::input::{GuestInput, GuestOutput};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// for raiko use
/// to support use case like raiko reads guest input from remote raiko
///

#[derive(Debug, Serialize, ToSchema, Deserialize)]
/// The response body of a proof request.
pub struct ProofResponse {
    #[schema(value_type = Option<GuestInput>)]
    /// The input of the prover.
    pub input: Option<GuestInput>,
    #[schema(value_type = Option<GuestOutputDoc>)]
    /// The output of the prover.
    pub output: Option<GuestOutput>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok { data: ProofResponse },
    Error { error: String, message: String },
}

#[cfg(test)]
mod test {
    use core::{fmt::Debug, str::FromStr};
    use std::u128;

    use anyhow::{anyhow, Error, Result};
    use raiko_lib::input::{
        ontake::BlockProposedV2, BlobProofType, BlockProposed, BlockProposedFork, GuestOutput,
        TaikoProverData,
    };
    use reth_primitives::{
        revm_primitives::{Address, Bytes, HashMap, B256, U256},
        Header, TransactionSigned, TxEip1559,
    };
    use serde::{Deserialize, Serialize};
    use serde_with::serde_as;

    pub type StorageEntry = (MptNode, Vec<U256>);

    use raiko_lib::{consts::ChainSpec, input::TaikoGuestInput, primitives::mpt::MptNode};
    use utoipa::ToSchema;

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]

    pub enum BlockProposedForkRef {
        #[default]
        Nothing,
        Hekla(BlockProposed),
        Ontake(BlockProposedV2Ref),
    }

    #[serde_as]
    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub struct TaikoGuestInputRef {
        /// header
        // pub l1_header: Header,
        // pub tx_data: Vec<u8>,
        pub anchor_tx: Option<TransactionSigned>,
        pub block_proposed: BlockProposedForkRef,
        // pub prover_data: TaikoProverData,
        // pub blob_commitment: Option<Vec<u8>>,
        // pub blob_proof: Option<Vec<u8>>,
        // pub blob_proof_type: BlobProofType,
    }

    #[serde_as]
    #[derive(Debug, Clone, Default, Deserialize, Serialize)]
    pub struct GuestInputRef {
        // /// Reth block
        // pub block: BlockV2,
        // /// The network to generate the proof for
        // pub chain_spec: ChainSpec,
        // /// Previous block header
        // pub parent_header: Header,
        // /// State trie of the parent block.
        // pub parent_state_trie: MptNode,
        // /// Maps each address with its storage trie and the used storage slots.
        // pub parent_storage: HashMap<Address, StorageEntry>,
        // /// The code of all unique contracts.
        // pub contracts: Vec<Bytes>,
        // /// List of at most 256 previous block headers
        // pub ancestor_headers: Vec<Header>,
        // // /// Taiko specific data
        pub taiko: TaikoGuestInputRef,
    }

    #[derive(Debug, Serialize, ToSchema, Deserialize)]
    /// The response body of a proof request.
    pub struct ProofResponse {
        #[schema(value_type = Option<GuestInput>)]
        /// The input of the prover.
        pub input: Option<GuestInputRef>,
        // #[schema(value_type = Option<GuestOutputDoc>)]
        // /// The output of the prover.
        // pub output: Option<GuestOutput>,
    }

    #[derive(Debug, Deserialize, Serialize, ToSchema)]
    // #[serde(tag = "status", rename_all = "lowercase")]
    #[serde(rename_all = "lowercase")]
    pub enum Status {
        Ok { data: ProofResponse },
        Error { error: String, message: String },
    }

    #[test]
    fn test_ser_der_status() {
        let mut tx = TxEip1559::default();
        tx.max_fee_per_gas = u128::MAX - 5;
        let anchor_tx = TransactionSigned {
            hash: Default::default(),
            signature: Default::default(),
            transaction: reth_primitives::Transaction::Eip1559(tx)
        };
        let input = GuestInputRef {
            // block: BlockV2::default(), 
            // chain_spec: ChainSpec::default(),
            // parent_header: Header::default(),
            // parent_state_trie: MptNode::default(),
            // parent_storage: HashMap::default(),
            // contracts: vec![Bytes::default()],
            // ancestor_headers: vec![Header::default()],
            taiko: TaikoGuestInputRef {
                block_proposed: BlockProposedForkRef::Ontake(BlockProposedV2Ref {
                    blockId: Default::default(),
                    meta: BlockMetadataV2Ref {
                        livenessBond: u128::MAX >> 33,
                    },
                }),
                anchor_tx: Some(anchor_tx),
            },
        };

        let status = Status::Ok {
            data: ProofResponse { input: Some(input) },
        };

        let serialized = serde_json::to_string(&status).unwrap();
        let deserialized: Status = serde_json::from_str(&serialized).unwrap();

        // assert_eq!(input, deserialized);
        print!("{:?}", deserialized);
    }

    #[test]
    fn test_ser_der() {
        let input = GuestInputRef {
            // block: BlockV2::default(),
            // chain_spec: ChainSpec::default(),
            // parent_header: Header::default(),
            // parent_state_trie: MptNode::default(),
            // parent_storage: HashMap::default(),
            // contracts: vec![Bytes::default()],
            // ancestor_headers: vec![Header::default()],
            taiko: TaikoGuestInputRef {
                block_proposed: BlockProposedForkRef::Ontake(BlockProposedV2Ref {
                    blockId: Default::default(),
                    meta: BlockMetadataV2Ref {
                        livenessBond: u128::MAX >> 33,
                    },
                }),
                ..Default::default()
            },
        };

        let serialized = serde_json::to_string(&input).unwrap();
        let deserialized: GuestInputRef = serde_json::from_str(&serialized).unwrap();

        // assert_eq!(input, deserialized);
        print!("{:?}", deserialized);
    }

    #[test]
    fn test_deser_files() {
        let json_str = include_str!("../../../response.log");
        let deserialized = serde_json::from_str::<Status>(json_str);
        println!("{:?}", deserialized);
    }

    use alloy_sol_types::sol;
    sol! {
        #[derive(Serialize, Deserialize, Debug)]
        struct SolRef {
            uint96 data;
        }

        #[derive(Debug, Default, Deserialize, Serialize)]
        struct BlockMetadataV2Ref {
            // bytes32 anchorBlockHash; // `_l1BlockHash` in TaikoL2's anchor tx.
            // bytes32 difficulty;
            // bytes32 blobHash;
            // bytes32 extraData;
            // address coinbase;
            // uint64 id;
            // uint32 gasLimit;
            // uint64 timestamp;
            // uint64 anchorBlockId; // `_l1BlockId` in TaikoL2's anchor tx.
            // uint16 minTier;
            // bool blobUsed;
            // bytes32 parentMetaHash;
            // address proposer;
            uint96 livenessBond;
            // // Time this block is proposed at, used to check proving window and cooldown window.
            // uint64 proposedAt;
            // // L1 block number, required/used by node/client.
            // uint64 proposedIn;
            // uint32 blobTxListOffset;
            // uint32 blobTxListLength;
            // uint8 blobIndex;
            // BaseFeeConfig baseFeeConfig;
        }

        #[derive(Debug, Default, Deserialize, Serialize)]
        event BlockProposedV2Ref(uint256 indexed blockId, BlockMetadataV2Ref meta);

    }

    #[derive(Serialize, Deserialize, Debug)]
    struct JsonRef {
        data: u128,
    }

    #[test]
    fn test_simple_u128_json() {
        let json_str = serde_json::to_string(&JsonRef {
            data: u128::MAX - 1,
        });
        println!("json_str = {:?}.", json_str);

        let json_obj = serde_json::from_str::<JsonRef>(&json_str.unwrap());
        println!("json_obj = {:?}.", json_obj);

        let test_json_str = "{\"data\": 125000000000000000000}";
        let json_obj = serde_json::from_str::<JsonRef>(test_json_str);
        println!("json_obj from str= {:?}.", json_obj);
    }

    #[test]
    fn test_simple_sol_u96_json() {
        let test_json_str = "{\"data\": 125000000000000000000}";
        let json_obj = serde_json::from_str::<SolRef>(test_json_str);
        println!("json_obj from str= {:?}.", json_obj);
    }
}
