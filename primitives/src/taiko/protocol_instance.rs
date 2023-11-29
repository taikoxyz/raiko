use std::{iter, str::FromStr};

use alloy_dyn_abi::DynSolValue;
use alloy_primitives::{Address, B256, U160, U256};
use alloy_sol_types::{sol, SolValue};
use serde::{
    de::{Error as DeError, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};

use crate::keccak;

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct EthDeposit {
        address recipient;
        #[serde(serialize_with = "serialize_amount")]
        #[serde(deserialize_with = "deserialize_amount")]
        uint96 amount;
        uint64 id;
    }

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

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockEvidence {
        BlockMetadata blockMetadata;
        bytes32 parentHash; // constrain: l2 parent hash
        bytes32 blockHash; // constrain: l2 block hash
        bytes32 signalRoot; // constrain: ??l2 service account storage root??
        bytes32 graffiti; // constrain: l2 block's graffiti
    }
}

fn serialize_amount<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if *value > (u64::MAX as u128) || *value < (u64::MIN as u128) {
        return value.to_string().serialize(serializer);
    }

    value.serialize(serializer)
}

fn deserialize_amount<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(AmountVisitor)
}

#[derive(Debug)]
struct AmountVisitor;
impl<'de> Visitor<'de> for AmountVisitor {
    type Value = u128;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        dbg!(self);
        formatter.write_str("expect to receive integer")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        v.parse().map_err(DeError::custom)
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(v as u128)
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(v as u128)
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

pub enum EvidenceType {
    Sgx {
        new_pubkey: String, // the evidence signature public key
    },
    PseZk,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolInstance {
    pub block_evidence: BlockEvidence,
    pub prover: Address,
}

impl ProtocolInstance {
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
            FixedBytes(self.block_evidence.blockMetadata.hash(), 32),
            FixedBytes(self.block_evidence.parentHash, 32),
            FixedBytes(self.block_evidence.blockHash, 32),
            FixedBytes(self.block_evidence.signalRoot, 32),
            FixedBytes(self.block_evidence.graffiti, 32),
            Address(self.prover),
        ];
        match evidence_type {
            EvidenceType::Sgx { new_pubkey } => {
                abi_encode_tuple.push(Bytes(hex::decode(new_pubkey).unwrap()));
            }
            EvidenceType::PseZk => {}
        };
        let input: Vec<u8> = Tuple(abi_encode_tuple).abi_encode();
        keccak::keccak(input).into()
    }
}
