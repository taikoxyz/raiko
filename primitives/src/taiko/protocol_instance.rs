use alloy_dyn_abi::DynSolValue;
use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{sol, SolEvent, SolValue, TopicList};
use anyhow::{Context, Result};
use ethers_core::types::{Log, H256};
use serde::{
    de::{Error as DeError, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};

use crate::{ethers::from_ethers_h256, keccak};

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
        bytes32 l1Hash; // slot 1
        bytes32 difficulty; // slot 2
        bytes32 blobHash; //or txListHash (if Blob not yet supported), // slot 3
        bytes32 extraData; // slot 4
        bytes32 depositsHash; // slot 5
        address coinbase; // L2 coinbase, // slot 6
        uint64 id;
        uint32 gasLimit;
        uint64 timestamp; // slot 7
        uint64 l1Height;
        uint24 txListByteOffset;
        uint24 txListByteSize;
        uint16 minTier;
        bool blobUsed;
        bytes32 parentMetaHash; // slot 8
    }

    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 signalRoot;
        bytes32 graffiti;
    }

    struct SgxGetSignedHash {
        Transition transition;
        address newInstance;
        address prover;
        bytes32 metaHash;
    }

    event BlockProposed(
        uint256 indexed blockId,
        address indexed prover,
        uint96 livenessBond,
        BlockMetadata meta,
        EthDeposit[] depositsProcessed
    );
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

pub fn filter_propose_block_event(
    logs: &[Log],
    block_id: U256,
) -> Result<Option<(H256, BlockMetadata)>> {
    for log in logs {
        if log.topics.len() != <<BlockProposed as SolEvent>::TopicList as TopicList>::COUNT {
            continue;
        }
        if from_ethers_h256(log.topics[0]) != BlockProposed::SIGNATURE_HASH {
            continue;
        }
        let topics = log.topics.iter().map(|topic| from_ethers_h256(*topic));
        let result = BlockProposed::decode_log(topics, &log.data, false);
        let block_proposed = result.context("decode log failed")?;
        if block_proposed.blockId == block_id {
            return Ok(log.transaction_hash.map(|h| (h, block_proposed.meta)));
        }
    }
    Ok(None)
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

pub enum EvidenceType {
    Sgx {
        new_pubkey: Address, // the evidence signature public key
    },
    PseZk,
}

pub struct ProtocolInstance {
    pub transition: Transition,
    pub block_metadata: BlockMetadata,
    pub prover: Address,
}

impl ProtocolInstance {
    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn hash(&self, evidence_type: EvidenceType) -> B256 {
        match evidence_type {
            EvidenceType::Sgx { new_pubkey } => {
                let meta_hash = keccak::keccak(self.block_metadata.abi_encode());
                let sgx_get_signed_hash = SgxGetSignedHash {
                    transition: self.transition.clone(),
                    newInstance: new_pubkey,
                    prover: self.prover,
                    metaHash: meta_hash.into(),
                };
                keccak::keccak(sgx_get_signed_hash.abi_encode()).into()
            }
            EvidenceType::PseZk => todo!(),
        }
    }
}

pub fn deposits_hash(deposits: &[EthDeposit]) -> B256 {
    keccak::keccak(deposits.abi_encode()).into()
}
