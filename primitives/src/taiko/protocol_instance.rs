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
        bytes32 l1Hash;
        bytes32 difficulty;
        bytes32 blobHash; //or txListHash (if Blob not yet supported)
        bytes32 extraData;
        bytes32 depositsHash;
        address coinbase; // L2 coinbase,
        uint64 id;
        uint32 gasLimit;
        uint64 timestamp;
        uint64 l1Height;
        uint16 minTier;
        bool blobUsed;
        bytes32 parentMetaHash;
        address sender;
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
        let input = "0x10d008bd0000000000000000000000000000000000000000000000000000000000002f9f00000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000380894baefdf28e45f30419001e78215567b8215281431787ac1a706f6016f1a65b64e3be53e286a55bfb60e04070ec7251aa78690b2f995b5f2f414841135ca7e701b5cfd91b5e034152a20065fd48bb38013f6cd3b300596a9917ae84c6d10f1b302e31382e302d64657600000000000000000000000000000000000000000000569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd000000000000000000000000bcd4042de499d14e55001ccbb24a551f3b9540960000000000000000000000000000000000000000000000000000000000002f9f0000000000000000000000000000000000000000000000000000000000e4e1c00000000000000000000000000000000000000000000000000000000065ecd69400000000000000000000000000000000000000000000000000000000000042f0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004b6000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000016e77188d0120bd1db76bf4e03e76df1a0d4a620c1eb002af000a2e56f499e2da000000000000000000000000bcd4042de499d14e55001ccbb24a551f3b954096be906e0793f4b05c3a3e7e6dfab4314e15d0ef0cb11204782fcf3b897db2085762179631c54a6fec15c378d5b1097af0d6a33334e0913b76fe30548571b763d5d532423b37ac146f984250a1ede580236e961d2e107cd8f9806558a2b45eb08b000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002a0000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000064ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000";

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
        let input = "0x10d008bd0000000000000000000000000000000000000000000000000000000000002f9f00000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000380894baefdf28e45f30419001e78215567b8215281431787ac1a706f6016f1a65b64e3be53e286a55bfb60e04070ec7251aa78690b2f995b5f2f414841135ca7e701b5cfd91b5e034152a20065fd48bb38013f6cd3b300596a9917ae84c6d10f1b302e31382e302d64657600000000000000000000000000000000000000000000569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd000000000000000000000000bcd4042de499d14e55001ccbb24a551f3b9540960000000000000000000000000000000000000000000000000000000000002f9f0000000000000000000000000000000000000000000000000000000000e4e1c00000000000000000000000000000000000000000000000000000000065ecd69400000000000000000000000000000000000000000000000000000000000042f0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004b6000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000016e77188d0120bd1db76bf4e03e76df1a0d4a620c1eb002af000a2e56f499e2da000000000000000000000000bcd4042de499d14e55001ccbb24a551f3b954096be906e0793f4b05c3a3e7e6dfab4314e15d0ef0cb11204782fcf3b897db2085762179631c54a6fec15c378d5b1097af0d6a33334e0913b76fe30548571b763d5d532423b37ac146f984250a1ede580236e961d2e107cd8f9806558a2b45eb08b000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002a0000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000064ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000";

        let input_data = hex::decode(&input[2..]).unwrap();
        let proveBlockCall { blockId: _, input } =
            proveBlockCall::abi_decode(&input_data, false).unwrap();
        let (meta, trans, _proof) =
            <(BlockMetadata, Transition, TierProof)>::abi_decode_params(&input, false).unwrap();
        let meta_hash: B256 = keccak::keccak(meta.abi_encode()).into();
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
        assert_eq!(
            hex::encode(pi_hash),
            "3ae20f46cb24a38add100414d48dd21496b29fb1a6d89fd9b015519bfc96752e"
        );
    }
}
