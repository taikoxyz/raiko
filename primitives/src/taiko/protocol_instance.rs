use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{sol, SolEvent, SolValue, TopicList};
use anyhow::{anyhow, Context, Result};
use ethers_core::types::{Log, H256};
use serde::{Deserialize, Serialize};

use crate::{ethers::from_ethers_h256, keccak};

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct EthDeposit {
        address recipient;
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

    #[derive(Debug)]
    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 signalRoot;
        bytes32 graffiti;
    }

    #[derive(Debug, Default, Clone, Deserialize, Serialize)]
    event BlockProposed(
        uint256 indexed blockId,
        address indexed prover,
        uint96 livenessBond,
        BlockMetadata meta,
        EthDeposit[] depositsProcessed
    );
}

// require equal with the assembled protocol instance and the block proposed event
pub fn assert_pi_and_bp(pi: &ProtocolInstance, bp: &BlockProposed) -> Result<()> {
    if pi.block_metadata.abi_encode() != bp.meta.abi_encode() {
        return Err(anyhow!("block metadata mismatch"));
    }
    Ok(())
}

pub fn filter_propose_block_event(
    logs: &[Log],
    block_id: U256,
) -> Result<Option<(H256, BlockProposed)>> {
    for log in logs {
        if log.topics.len() != <<BlockProposed as SolEvent>::TopicList as TopicList>::COUNT {
            continue;
        }
        if from_ethers_h256(log.topics[0]) != BlockProposed::SIGNATURE_HASH {
            continue;
        }
        let topics = log.topics.iter().map(|topic| from_ethers_h256(*topic));
        let result = BlockProposed::decode_log(topics, &log.data, false);
        let block_proposed = result.with_context(|| "decode log failed")?;
        if block_proposed.blockId == block_id {
            return Ok(log.transaction_hash.map(|h| (h, block_proposed)));
        }
    }
    Ok(None)
}

#[derive(Debug)]
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
                keccak::keccak(
                    (self.transition.clone(), new_pubkey, self.prover, meta_hash).abi_encode(),
                )
                .into()
            }
            EvidenceType::PseZk => todo!(),
        }
    }
}

pub fn deposits_hash(deposits: &[EthDeposit]) -> B256 {
    keccak::keccak(deposits.abi_encode()).into()
}
