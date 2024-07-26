use alloy_primitives::{Address, TxHash, B256};
use alloy_sol_types::SolValue;
use anyhow::{ensure, Result};
use reth_primitives::{Header, U256};

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{
    consts::{SupportedChainSpecs, VerifierType},
    input::{
        ontake::BlockMetadataV2, BlobProofType, BlockMetadata, BlockProposedFork, EthDeposit,
        GuestInput, Transition,
    },
    primitives::{
        eip4844::{self, commitment_to_version_hash},
        keccak::keccak,
    },
};
use reth_evm_ethereum::taiko::ANCHOR_GAS_LIMIT;

#[derive(Debug, Clone)]
pub enum BlockMetaDataFork {
    Hekla(BlockMetadata),
    Ontake(BlockMetadataV2),
}

impl BlockMetaDataFork {
    fn from(input: &GuestInput, header: &Header, tx_list_hash: B256) -> Self {
        match &input.taiko.block_proposed {
            BlockProposedFork::Nothing => unimplemented!("no block proposed"),
            BlockProposedFork::Hekla(block_proposed) => BlockMetaDataFork::Hekla(BlockMetadata {
                l1Hash: input.taiko.l1_header.hash_slow(),
                difficulty: block_proposed.meta.difficulty,
                blobHash: tx_list_hash,
                extraData: bytes_to_bytes32(&header.extra_data).into(),
                depositsHash: keccak(Vec::<EthDeposit>::new().abi_encode()).into(),
                coinbase: header.beneficiary,
                id: header.number,
                gasLimit: (header.gas_limit
                    - if input.chain_spec.is_taiko() {
                        ANCHOR_GAS_LIMIT
                    } else {
                        0
                    }) as u32,
                timestamp: header.timestamp,
                l1Height: input.taiko.l1_header.number,
                minTier: block_proposed.meta.minTier,
                blobUsed: block_proposed.meta.blobUsed,
                parentMetaHash: block_proposed.meta.parentMetaHash,
                sender: block_proposed.meta.sender,
            }),
            BlockProposedFork::Ontake(block_proposed_v2) => {
                BlockMetaDataFork::Ontake(BlockMetadataV2 {
                    anchorBlockHash: input.taiko.l1_header.hash_slow(),
                    difficulty: block_proposed_v2.meta.difficulty,
                    blobHash: tx_list_hash,
                    extraData: bytes_to_bytes32(&header.extra_data).into(),
                    coinbase: header.beneficiary,
                    id: header.number,
                    gasLimit: (header.gas_limit
                        - if input.chain_spec.is_taiko() {
                            ANCHOR_GAS_LIMIT
                        } else {
                            0
                        }) as u32,
                    timestamp: header.timestamp,
                    anchorBlockId: input.taiko.l1_header.number,
                    minTier: block_proposed_v2.meta.minTier,
                    blobUsed: block_proposed_v2.meta.blobUsed,
                    parentMetaHash: block_proposed_v2.meta.parentMetaHash,
                    proposer: block_proposed_v2.meta.proposer,
                    livenessBond: block_proposed_v2.meta.livenessBond,
                    proposedAt: block_proposed_v2.meta.proposedAt,
                    proposedIn: block_proposed_v2.meta.proposedIn,
                    blobTxListOffset: block_proposed_v2.meta.blobTxListOffset,
                    blobTxListLength: block_proposed_v2.meta.blobTxListLength,
                    blobIndex: block_proposed_v2.meta.blobIndex,
                    basefeeSharingPctg: block_proposed_v2.meta.basefeeSharingPctg,
                })
            }
        }
    }

    fn match_block_proposal(&self, other: &BlockProposedFork) -> bool {
        match (self, other) {
            (BlockMetaDataFork::Hekla(a), BlockProposedFork::Hekla(b)) => {
                a.abi_encode() == b.meta.abi_encode()
            }
            (BlockMetaDataFork::Ontake(a), BlockProposedFork::Ontake(b)) => {
                a.abi_encode() == b.meta.abi_encode()
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProtocolInstance {
    pub transition: Transition,
    pub block_metadata: BlockMetaDataFork,
    pub prover: Address,
    pub sgx_instance: Address, // only used for SGX
    pub chain_id: u64,
    pub verifier_address: Address,
    pub proof_of_equivalence: (U256, U256),
}

impl ProtocolInstance {
    pub fn new(input: &GuestInput, header: &Header, proof_type: VerifierType) -> Result<Self> {
        let blob_used = input.taiko.block_proposed.blob_used();
        // If blob is used, tx_list_hash is the commitment to the blob
        // and we need to verify the blob hash matches the blob data.
        // If we need to compute the proof of equivalence this data will be set.
        // Otherwise the proof_of_equivalence is 0
        let mut proof_of_equivalence = (U256::ZERO, U256::ZERO);
        let tx_list_hash = if blob_used {
            let commitment = input
                .taiko
                .blob_commitment
                .as_ref()
                .expect("no blob commitment");
            let versioned_hash =
                commitment_to_version_hash(&commitment.clone().try_into().unwrap());
            match get_blob_proof_type(proof_type, input.taiko.blob_proof_type.clone()) {
                crate::input::BlobProofType::ProofOfEquivalence => {
                    let points =
                        eip4844::proof_of_equivalence(&input.taiko.tx_data, &versioned_hash)?;
                    proof_of_equivalence =
                        (U256::from_le_bytes(points.0), U256::from_le_bytes(points.1));
                }
                crate::input::BlobProofType::ProofOfCommitment => {
                    ensure!(
                        commitment == &eip4844::calc_kzg_proof_commitment(&input.taiko.tx_data)?
                    );
                }
            };
            versioned_hash
        } else {
            TxHash::from(keccak(input.taiko.tx_data.as_slice()))
        };

        // If the passed in chain spec contains a known chain id, the chain spec NEEDS to match the
        // one we expect, because the prover could otherwise just fill in any values.
        // The chain id is used because that is the value that is put onchain,
        // and so all other chain data needs to be derived from it.
        // For unknown chain ids we just skip this check so that tests using test data can still pass.
        // TODO: we should probably split things up in critical and non-critical parts
        // in the chain spec itself so we don't have to manually all the ones we have to care about.
        if let Some(verified_chain_spec) =
            SupportedChainSpecs::default().get_chain_spec_with_chain_id(input.chain_spec.chain_id)
        {
            ensure!(
                input.chain_spec.max_spec_id == verified_chain_spec.max_spec_id,
                "unexpected max_spec_id"
            );
            ensure!(
                input.chain_spec.hard_forks == verified_chain_spec.hard_forks,
                "unexpected hard_forks"
            );
            ensure!(
                input.chain_spec.eip_1559_constants == verified_chain_spec.eip_1559_constants,
                "unexpected eip_1559_constants"
            );
            ensure!(
                input.chain_spec.l1_contract == verified_chain_spec.l1_contract,
                "unexpected l1_contract"
            );
            ensure!(
                input.chain_spec.l2_contract == verified_chain_spec.l2_contract,
                "unexpected l2_contract"
            );
            ensure!(
                input.chain_spec.is_taiko == verified_chain_spec.is_taiko,
                "unexpected eip_1559_constants"
            );
        }

        let verifier_address = input
            .chain_spec
            .get_fork_verifier_address(input.taiko.block_proposed.block_number(), proof_type)?;

        let pi = ProtocolInstance {
            transition: Transition {
                parentHash: header.parent_hash,
                blockHash: header.hash_slow(),
                stateRoot: header.state_root,
                graffiti: input.taiko.prover_data.graffiti,
            },
            block_metadata: BlockMetaDataFork::from(input, header, tx_list_hash),
            sgx_instance: Address::default(),
            prover: input.taiko.prover_data.prover,
            chain_id: input.chain_spec.chain_id,
            verifier_address,
            proof_of_equivalence,
        };

        // Sanity check
        if input.chain_spec.is_taiko() {
            ensure!(
                pi.block_metadata
                    .match_block_proposal(&input.taiko.block_proposed),
                format!(
                    "block hash mismatch, expected: {:?}, got: {:?}",
                    input.taiko.block_proposed, pi.block_metadata
                )
            );
        }

        Ok(pi)
    }

    pub fn sgx_instance(mut self, instance: Address) -> Self {
        self.sgx_instance = instance;
        self
    }

    pub fn meta_hash(&self) -> B256 {
        match self.block_metadata {
            BlockMetaDataFork::Hekla(ref meta) => keccak(meta.abi_encode()).into(),
            BlockMetaDataFork::Ontake(ref meta) => keccak(meta.abi_encode()).into(),
        }
    }

    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn instance_hash(&self) -> B256 {
        // packages/protocol/contracts/verifiers/libs/LibPublicInput.sol
        // "VERIFY_PROOF", _chainId, _verifierContract, _tran, _newInstance, _prover, _metaHash
        let data = (
            "VERIFY_PROOF",
            self.chain_id,
            self.verifier_address,
            self.transition.clone(),
            self.sgx_instance,
            self.prover,
            self.meta_hash(),
            #[cfg(feature = "proof_of_equivalence")]
            self.proof_of_equivalence,
        )
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect::<Vec<u8>>();
        keccak(data).into()
    }
}

// Make sure the verifier supports the blob proof type
fn get_blob_proof_type(
    proof_type: VerifierType,
    blob_proof_type_hint: BlobProofType,
) -> BlobProofType {
    if cfg!(feature = "proof_of_equivalence") {
        match proof_type {
            VerifierType::None => blob_proof_type_hint,
            VerifierType::SGX => BlobProofType::ProofOfCommitment,
            VerifierType::SP1 => BlobProofType::ProofOfEquivalence,
            VerifierType::RISC0 => BlobProofType::ProofOfEquivalence,
        }
    } else {
        BlobProofType::ProofOfCommitment
    }
}

fn bytes_to_bytes32(input: &[u8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    let len = core::cmp::min(input.len(), 32);
    bytes[..len].copy_from_slice(&input[..len]);
    bytes
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};
    use alloy_sol_types::SolCall;

    use super::*;
    use crate::{
        input::{proveBlockCall, TierProof},
        primitives::keccak,
    };

    #[test]
    fn bytes_to_bytes32_test() {
        let input = "";
        let byte = bytes_to_bytes32(input.as_bytes());
        assert_eq!(
            byte,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ]
        );
    }

    #[test]
    fn test_calc_eip712_pi_hash() {
        let trans = Transition {
            parentHash: b256!("07828133348460fab349c7e0e9fd8e08555cba34b34f215ffc846bfbce0e8f52"),
            blockHash: b256!("e2105909de032b913abfa4c8b6101f9863d82be109ef32890b771ae214784efa"),
            stateRoot: b256!("abbd12b3bcb836b024c413bb8c9f58f5bb626d6d835f5554a8240933e40b2d3b"),
            graffiti: b256!("0000000000000000000000000000000000000000000000000000000000000000"),
        };
        let meta_hash = b256!("9608088f69e586867154a693565b4f3234f26f82d44ef43fb99fd774e7266024");
        let proof_of_equivalence = ([0u8; 32], [0u8; 32]);

        let pi_hash = keccak::keccak(
            (
                "VERIFY_PROOF",
                167001u64,
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
                trans.clone(),
                address!("741E45D08C70c1C232802711bBFe1B7C0E1acc55"),
                address!("70997970C51812dc3A010C7d01b50e0d17dc79C8"),
                meta_hash,
                proof_of_equivalence,
            )
                .abi_encode()
                .iter()
                .cloned()
                .skip(32)
                .collect::<Vec<u8>>(),
        );
        assert_eq!(
            hex::encode(pi_hash),
            "dc1696a5289616fa5eaa9b6ce97d53765b79db948caedb6887f21a26e4c29511"
        );
    }

    // TODO: update proof_of_equivalence
    #[test]
    fn test_eip712_pi_hash() {
        let input = "10d008bd000000000000000000000000000000000000000000000000000000000000004900000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000340689c98d83627e8749504eb6effbc2b08408183f11211bbf8bd281727b16255e6b3f8ee61d80cd7d30cdde9aa49acac0b82264a6b0f992139398e95636e501fd80189249f72753bd6c715511cc61facdec4781d4ecb1d028dafdff4a0827d7d53302e31382e302d64657600000000000000000000000000000000000000000000569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd00000000000000000000000016700100000000000000000000000000000100010000000000000000000000000000000000000000000000000000000000000049000000000000000000000000000000000000000000000000000000000e4e1c000000000000000000000000000000000000000000000000000000000065f94010000000000000000000000000000000000000000000000000000000000000036000000000000000000000000000000000000000000000000000000000000000640000000000000000000000000000000000000000000000000000000000000001fdbdc45da60168ddf29b246eb9e0a2e612a670f671c6d3aafdfdac21f86b4bca0000000000000000000000003c44cdddb6a900fa2b585dd299e03d12fa4293bcaf73b06ee94a454236314610c55e053df3af4402081df52c9ff2692349a6b497bc17a6706bc1cf4c363e800d2133d0d143363871d9c17b8fc5cf6d3cfd585bc80730a40cf8d8186241d45e19785c117956de919999d50e473aaa794b8fd4097000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000260000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000064ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000";
        let input_data = hex::decode(input).unwrap();
        let proveBlockCall { blockId: _, input } =
            proveBlockCall::abi_decode(&input_data, false).unwrap();
        let (meta, trans, _proof) =
            <(BlockMetadata, Transition, TierProof)>::abi_decode_params(&input, false).unwrap();
        let meta_hash: B256 = keccak::keccak(meta.abi_encode()).into();
        let proof_of_equivalence = ([0u8; 32], [0u8; 32]);

        let pi_hash = keccak::keccak(
            (
                "VERIFY_PROOF",
                10086u64,
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
                trans.clone(),
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
                meta_hash,
                proof_of_equivalence,
            )
                .abi_encode()
                .iter()
                .cloned()
                .skip(32)
                .collect::<Vec<u8>>(),
        );
        assert_eq!(
            hex::encode(pi_hash),
            "8b0e2833f7bae47f6886e5f172d90b12e330485bfe366d8ed4d53b2114d47e68"
        );
    }
}
