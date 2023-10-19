use std::iter;

use crate::keccak;
use alloy_sol_types::{sol, SolValue};
use serde::{Deserialize, Serialize};

use alloy_primitives::{B256, U160, U256};

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct EthDeposit {
        address recipient;
        uint96 amount;
        uint64 id;
    }
}

// pub struct EthDeposit {
//     pub recipient: Address,
//     pub amount: U96, // u96
//     pub id: U64,
// }

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockMetadata {
        uint64 id;
        uint64 timestamp;
        uint64 l1Height;
        bytes32 l1Hash;
        bytes32 mixHash;
        bytes32 txListHash;
        uint24 txListByteStart;
        uint24 txListByteEnd;
        uint32 gasLimit;
        address proposer;
        EthDeposit[] depositsProcessed;
    }
}

// /// Taiko l1 meta hash
// #[derive(Debug, Clone, Default, Deserialize, Serialize)]
// pub struct MetaData {
//     /// meta id
//     pub id: U64,
//     /// meta timestamp
//     pub timestamp: U64,
//     /// l1 block height
//     pub l1_height: U64,
//     /// l1 block hash
//     pub l1_hash: BlockHash,
//     /// l1 block mix hash
//     pub mix_hash: B256,
//     /// tx list hash
//     pub tx_list_hash: B256,
//     /// tx list byte start
//     pub tx_list_byte_start: U24, // u24
//     /// tx list byte end
//     pub tx_list_byte_end: U24, // u24
//     /// gas limit
//     pub gas_limit: U32,
//     /// beneficiary
//     pub proposer: Address,

//     /// deposits processed
//     pub deposits_processed: Vec<EthDeposit>,
// }

impl BlockMetadata {
    pub fn hash(&self) -> B256 {
        let field0 = U256::from(self.id) << 192
            | U256::from(self.timestamp) << 128
            | U256::from(self.l1Height) << 64;
        let field1 = self.l1Hash;
        let field2 = self.mixHash;
        let field3 = keccak::keccak(self.depositsProcessed.abi_encode());
        let field4 = self.txListHash;
        let proposer: U160 = self.proposer.into();
        let field5 = U256::from(self.txListByteStart) << 232
            | U256::from(self.txListByteEnd) << 208
            | U256::from(self.gasLimit) << 176
            | U256::from(proposer) << 16;

        let input: Vec<u8> = iter::empty()
            .chain(field0.to_be_bytes_vec())
            .chain(field1)
            .chain(field2)
            .chain(field3)
            .chain(field4)
            .chain(field5.to_be_bytes_vec())
            .collect();
        keccak::keccak(input).into()
    }
}

// function hashMetadata(TaikoData.BlockMetadata memory meta)
//     internal
//     pure
//     returns (bytes32 hash)
// {
//     uint256[6] memory inputs;

//     inputs[0] = (uint256(meta.id) << 192) | (uint256(meta.timestamp) << 128)
//         | (uint256(meta.l1Height) << 64);

//     inputs[1] = uint256(meta.l1Hash);
//     inputs[2] = uint256(meta.mixHash);
//     inputs[3] =
//         uint256(LibDepositing.hashEthDeposits(meta.depositsProcessed));
//     inputs[4] = uint256(meta.txListHash);

//     inputs[5] = (uint256(meta.txListByteStart) << 232)
//         | (uint256(meta.txListByteEnd) << 208) //
//         | (uint256(meta.gasLimit) << 176)
//         | (uint256(uint160(meta.proposer)) << 16);

//     assembly {
//         hash := keccak256(inputs, mul(6, 32))
//     }
// }

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockEvidence {
        bytes32 metaHash;
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 signalRoot;
        bytes32 graffiti;
        address prover;
        bytes proofs;
    }

}

pub type ProtocolInstance = BlockEvidence;

// /// Taiko witness
// #[derive(Debug, Clone, Default, Deserialize, Serialize)]
// pub struct ProtocolInstance {
//     /// meta hash from l1
//     pub meta_data: MetaData,
//     /// block hash value
//     pub block_hash: BlockHash,
//     /// the parent block hash
//     pub parent_hash: BlockHash,
//     /// signal root
//     pub signal_root: B256,
//     /// extra message
//     pub graffiti: B256,
//     /// Prover address
//     pub prover: Address,
// }
