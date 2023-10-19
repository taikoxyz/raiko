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

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockMetadata {
        bytes32 l1Hash; // constrain: anchor call
        bytes32 difficulty; // constrain: l2 block's difficulty
        bytes32 txListHash; // constrain: l2 txlist
        bytes32 extraData; // constrain: l2 block's extra data
        uint64 id; // constrain: l2 block's number
        uint64 timestamp; // constrain: l2 block's timestamp
        uint64 l1Height; // constrain: anchor
        uint32 gasLimit; // constrain: l2 block's gas limit - anchor gas limit
        address coinbase; // constrain: L2 coinbase
        EthDeposit[] depositsProcessed; // constrain: l2 withdraw root
    }
}

impl BlockMetadata {
    // FIXME
    pub fn hash(&self) -> B256 {
        // let field0 = U256::from(self.id) << 192
        //     | U256::from(self.timestamp) << 128
        //     | U256::from(self.l1Height) << 64;
        // let field1 = self.l1Hash;
        // let field2 = self.mixHash;
        // let field3 = keccak::keccak(self.depositsProcessed.abi_encode());
        // let field4 = self.txListHash;
        // let proposer: U160 = self.proposer.into();
        // let field5 = U256::from(self.txListByteStart) << 232
        //     | U256::from(self.txListByteEnd) << 208
        //     | U256::from(self.gasLimit) << 176
        //     | U256::from(proposer) << 16;

        // let input: Vec<u8> = iter::empty()
        //     .chain(field0.to_be_bytes_vec())
        //     .chain(field1)
        //     .chain(field2)
        //     .chain(field3)
        //     .chain(field4)
        //     .chain(field5.to_be_bytes_vec())
        //     .collect();
        // keccak::keccak(input).into()
        todo!()
    }
}

// function hashMetadata(TaikoData.BlockMetadata memory meta)
//         internal
//         pure
//         returns (bytes32 hash)
//     {
//         uint256[7] memory inputs;
//         inputs[0] = uint256(meta.l1Hash);
//         inputs[1] = uint256(meta.difficulty);
//         inputs[2] = uint256(meta.txListHash);
//         inputs[3] = uint256(meta.extraData);
//         inputs[4] = (uint256(meta.id)) | (uint256(meta.timestamp) << 64)
//             | (uint256(meta.l1Height) << 128) | (uint256(meta.gasLimit) << 192);
//         inputs[5] = uint256(uint160(meta.coinbase));
//         inputs[6] = uint256(keccak256(abi.encode(meta.depositsProcessed)));

//         assembly {
//             hash := keccak256(inputs, 224 /*mul(7, 32)*/ )
//         }
//     }

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockEvidence {
        bytes32 metaHash;
        bytes32 parentHash; // constrain: l2 parent hash
        bytes32 blockHash; // constrain: l2 block hash
        bytes32 signalRoot; // constrain: ??l2 service account storage root??
        // l2 signal service account verify? constant?
        // 0x1000777700000000000000000000000000000007
        // https://github.com/taikoxyz/taiko-mono/blob/contestable-zkrollup/packages/protocol/contracts/common/AddressManager.sol
        bytes32 graffiti; // constrain: l2 block's graffiti
    }
}

pub type ProtocolInstance = BlockEvidence;

// l1 signal root: a5-testnet 0xcD5e2bebd3DfE46e4BF96aE2ac7B89B22cc6a982
