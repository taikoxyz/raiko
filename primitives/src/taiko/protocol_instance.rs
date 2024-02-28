use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{sol, SolEvent, SolValue, TopicList};
use anyhow::{Context, Result};
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
        bytes32 stateRoot;
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

    #[derive(Debug)]
    struct TierProof {
        uint16 tier;
        bytes data;
    }

    function proveBlock(uint64 blockId, bytes calldata input) {}
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

#[derive(Debug)]
pub struct ProtocolInstance {
    pub transition: Transition,
    pub block_metadata: BlockMetadata,
    pub prover: Address,
    pub chain_id: u64,
    pub sgx_verifier_address: Address,
}

impl ProtocolInstance {
    pub fn meta_hash(&self) -> B256 {
        keccak::keccak(self.block_metadata.abi_encode()).into()
    }

    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn hash(&self, evidence_type: EvidenceType) -> B256 {
        println!("chain_id: {:?}", self.chain_id);
        println!("sgx_verifier_address: {:?}", self.sgx_verifier_address);
        println!("transition: {:?}", self.transition);
        println!("prover: {:?}", self.prover);
        println!("meta_hash: {:?}", self.meta_hash());
        match evidence_type {
            EvidenceType::Sgx { new_pubkey } => keccak::keccak(
                (
                    "VERIFY_PROOF",
                    self.chain_id,
                    self.sgx_verifier_address,
                    self.transition.clone(),
                    new_pubkey,
                    self.prover,
                    self.meta_hash(),
                )
                    .abi_encode()
                    .iter()
                    .cloned()
                    .skip(32)
                    .collect::<Vec<u8>>(),
            )
            .into(),
            EvidenceType::PseZk => todo!(),
        }
    }
}

pub fn deposits_hash(deposits: &[EthDeposit]) -> B256 {
    keccak::keccak(deposits.abi_encode()).into()
}

#[cfg(test)]
mod tests {
    use alloy_sol_types::SolCall;
    use hex::FromHex;

    use super::*;
    #[test]
    fn test_prove_block_call() {
        let input = "0x10d008bd000000000000000000000000000000000000000000000000000000000000299e0000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000034057a97bd6f6930af5ca9e7caf48e663588755b690e9de0f82486416960135939559b91a6700c8af9442fe68f4339066d1d7858263c6be97ebcaca787ef70b1a7f8be37f1ab1fe1209f525f7cbced8a86ed49d1813849896c99835628f8eea703b302e31382e302d64657600000000000000000000000000000000000000000000569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd000000000000000000000000e1e210594771824dad216568b91c9cb4ceed361c000000000000000000000000000000000000000000000000000000000000299e0000000000000000000000000000000000000000000000000000000000e4e1c00000000000000000000000000000000000000000000000000000000065a63e6400000000000000000000000000000000000000000000000000000000000b6785000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000056220000000000000000000000000000000000000000000000000000000000000064000000000000000000000000000000000000000000000000000000000000000012d5f89f4195325e38f76ac324b08c34ab0c5c9ec430fc00dd967aa44b0bd05c11a7c619d13210437142d7adae4025ee65581228d0a8ed7a0df022634b2f1feadb23b17eaa3a5d3a7cfede2fa7d1653ac512117963c9fbe5f2df6a9dd555041ff20f4e661443b23d0c39ddbbb2725002cd2f7d5edb84d1c1eed9d8c71ddeba300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000028000000000000000000000000000000000000000000000000000000000000000c800000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000059000000000041035896fb7ccbed43b0fd70a82758535f3aa70e317bc173b815f18c416274d39cdd4918013cb12ccffc959700b8ae824b4a421d462c6fa19e28bdc64d6f753d978e0e76c33ce84aadfa19b68163c99dc62a631b00000000000000";

        let input_data = hex::decode(&input[2..]).unwrap();
        let proveBlockCall { blockId, input } =
            proveBlockCall::abi_decode(&input_data, false).unwrap();
        println!("blockId: {}", blockId);
        let (meta, trans, proof) =
            <(BlockMetadata, Transition, TierProof)>::abi_decode_params(&input, false).unwrap();
        println!("meta: {:?}", meta);
        let meta_hash: B256 = keccak::keccak(meta.abi_encode()).into();
        println!("meta_hash: {:?}", meta_hash);
        println!("trans: {:?}", trans);
        println!("proof: {:?}", proof.tier);
        println!("proof: {:?}", hex::encode(proof.data));
    }

    #[test]
    fn test_calc_eip712_pi_hash() {
        let trans = Transition {
            parentHash: B256::from_hex(
                "07828133348460fab349c7e0e9fd8e08555cba34b34f215ffc846bfbce0e8f52",
            )
            .unwrap(),
            blockHash: B256::from_hex(
                "e2105909de032b913abfa4c8b6101f9863d82be109ef32890b771ae214784efa",
            )
            .unwrap(),
            stateRoot: B256::from_hex(
                "abbd12b3bcb836b024c413bb8c9f58f5bb626d6d835f5554a8240933e40b2d3b",
            )
            .unwrap(),
            graffiti: B256::from_hex(
                "0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        };
        let meta_hash =
            B256::from_hex("9608088f69e586867154a693565b4f3234f26f82d44ef43fb99fd774e7266024")
                .unwrap();
        let pi_hash = keccak::keccak(
            (
                "VERIFY_PROOF",
                167001u64,
                Address::from_hex("0x4F3F0D5B22338f1f991a1a9686C7171389C97Ff7").unwrap(),
                trans.clone(),
                Address::from_hex("0x741E45D08C70c1C232802711bBFe1B7C0E1acc55").unwrap(),
                Address::from_hex("0x70997970C51812dc3A010C7d01b50e0d17dc79C8").unwrap(),
                meta_hash,
            )
                .abi_encode()
                .iter()
                .cloned()
                .skip(32)
                .collect::<Vec<u8>>(),
        );
        // println!("pi_hash: {:?}", hex::encode(pi_hash));
        assert_eq!(
            hex::encode(pi_hash),
            "4a7ba84010036277836eaf99acbbc10dc5d8ee9063e2e3c5be5e8be39ceba8ae"
        );
    }

    #[test]
    fn test_eip712_pi_hash() {
        let input = "0x10d008bd000000000000000000000000000000000000000000000000000000000000299e0000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000034057a97bd6f6930af5ca9e7caf48e663588755b690e9de0f82486416960135939559b91a6700c8af9442fe68f4339066d1d7858263c6be97ebcaca787ef70b1a7f8be37f1ab1fe1209f525f7cbced8a86ed49d1813849896c99835628f8eea703b302e31382e302d64657600000000000000000000000000000000000000000000569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd000000000000000000000000e1e210594771824dad216568b91c9cb4ceed361c000000000000000000000000000000000000000000000000000000000000299e0000000000000000000000000000000000000000000000000000000000e4e1c00000000000000000000000000000000000000000000000000000000065a63e6400000000000000000000000000000000000000000000000000000000000b6785000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000056220000000000000000000000000000000000000000000000000000000000000064000000000000000000000000000000000000000000000000000000000000000012d5f89f4195325e38f76ac324b08c34ab0c5c9ec430fc00dd967aa44b0bd05c11a7c619d13210437142d7adae4025ee65581228d0a8ed7a0df022634b2f1feadb23b17eaa3a5d3a7cfede2fa7d1653ac512117963c9fbe5f2df6a9dd555041ff20f4e661443b23d0c39ddbbb2725002cd2f7d5edb84d1c1eed9d8c71ddeba300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000028000000000000000000000000000000000000000000000000000000000000000c800000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000059000000000041035896fb7ccbed43b0fd70a82758535f3aa70e317bc173b815f18c416274d39cdd4918013cb12ccffc959700b8ae824b4a421d462c6fa19e28bdc64d6f753d978e0e76c33ce84aadfa19b68163c99dc62a631b00000000000000";

        let input_data = hex::decode(&input[2..]).unwrap();
        let proveBlockCall { blockId: _, input } =
            proveBlockCall::abi_decode(&input_data, false).unwrap();
        let (meta, trans, _proof) =
            <(BlockMetadata, Transition, TierProof)>::abi_decode_params(&input, false).unwrap();
        println!("trans: {:?}", trans);
        let meta_hash: B256 = keccak::keccak(meta.abi_encode()).into();
        println!("meta_hash: {:?}", meta_hash);

        let pi_hash = keccak::keccak(
            (
                "VERIFY_PROOF",
                10086u64,
                Address::from_hex("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7").unwrap(),
                trans.clone(),
                Address::from_hex("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7").unwrap(),
                Address::from_hex("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7").unwrap(),
                meta_hash,
            )
                .abi_encode()
                .iter()
                .cloned()
                .skip(32)
                .collect::<Vec<u8>>(),
        );
        // println!("pi_hash: {:?}", hex::encode(pi_hash));
        assert_eq!(
            hex::encode(pi_hash),
            "54b29e9a09c207a2677346b59ce63e786690fc4944a7318fff08ae25d1ba8c9e"
        );
    }
}
