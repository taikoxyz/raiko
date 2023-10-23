use crate::keccak;
use alloy_dyn_abi::DynSolValue;
use alloy_primitives::{Address, B256, U160, U256};
use alloy_sol_types::{sol, SolValue};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{iter, str::FromStr};

pub static L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0xcD5e2bebd3DfE46e4BF96aE2ac7B89B22cc6a982")
        .expect("invalid l1 signal service")
});

pub static L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x1000777700000000000000000000000000000007")
        .expect("invalid l2 signal service")
});

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
    pub fn withdraws_root(&self) -> B256 {
        // FIXME: mpt root
        keccak::keccak(self.depositsProcessed.abi_encode()).into()
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
    pub fn hash(&self) -> B256 {
        let field0 = self.l1Hash;
        let field1 = self.difficulty;
        let field2 = self.txListHash;
        let field3 = self.extraData;
        let field4 = U256::from(self.id)
            | U256::from(self.timestamp) << 64
            | U256::from(self.l1Height) << 128
            | U256::from(self.gasLimit) << 192;
        let coinbase: U160 = self.coinbase.into();
        let field5 = U256::from(coinbase);
        let field6 = keccak::keccak(self.depositsProcessed.abi_encode());
        let input: Vec<u8> = iter::empty()
            .chain(field0)
            .chain(field1)
            .chain(field2)
            .chain(field3)
            .chain(field4.to_be_bytes_vec())
            .chain(field5.to_be_bytes_vec())
            .chain(field6)
            .collect();
        keccak::keccak(input).into()
    }
}

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockEvidence {
        BlockMetadata blockMetadata;
        bytes32 parentHash; // constrain: l2 parent hash
        bytes32 blockHash; // constrain: l2 block hash
        bytes32 signalRoot; // constrain: ??l2 service account storage root??
        bytes32 graffiti; // constrain: l2 block's graffiti
    }
}

pub enum EvidenceType {
    Sgx {
        prover: Address,
        new_pubkey: Address, // the evidence signature public key
    },
    PseZk {
        prover: Address,
    },
}

impl BlockEvidence {
    // keccak256(
    //     abi.encode(
    //         evidence.metaHash,
    //         evidence.parentHash,
    //         evidence.blockHash,
    //         evidence.signalRoot,
    //         evidence.graffiti,
    //         assignedProver,
    //         newPubKey
    //     )
    // );
    pub fn hash(&self, evidence_type: EvidenceType) -> B256 {
        use DynSolValue::*;
        let mut abi_encode_tuple = vec![
            FixedBytes(self.blockMetadata.hash(), 32),
            FixedBytes(self.parentHash, 32),
            FixedBytes(self.blockHash, 32),
            FixedBytes(self.signalRoot, 32),
            FixedBytes(self.graffiti, 32),
        ];
        match evidence_type {
            EvidenceType::Sgx { prover, new_pubkey } => {
                abi_encode_tuple.extend(vec![Address(prover), Address(new_pubkey)]);
            }
            EvidenceType::PseZk { prover } => {
                abi_encode_tuple.push(Address(prover));
            }
        };
        let input: Vec<u8> = Tuple(abi_encode_tuple).abi_encode();
        keccak::keccak(input).into()
    }
}

pub type ProtocolInstance = BlockEvidence;
