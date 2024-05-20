use alloy_consensus::Header as AlloyConsensusHeader;
use alloy_primitives::{Address, TxHash, B256};
use alloy_sol_types::SolValue;
use anyhow::{ensure, Result};
use c_kzg::{Blob, KzgCommitment, KzgSettings};
use raiko_primitives::keccak::keccak;
use sha2::{Digest as _, Sha256};

use super::utils::ANCHOR_GAS_LIMIT;
#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{
    consts::SupportedChainSpecs,
    input::{BlockMetadata, EthDeposit, GuestInput, Transition},
    utils::HeaderHasher,
};

const KZG_TRUST_SETUP_DATA: &[u8] = include_bytes!("../../kzg_settings_raw.bin");

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
        keccak(self.block_metadata.abi_encode()).into()
    }

    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn instance_hash(&self, evidence_type: &EvidenceType) -> B256 {
        match evidence_type {
            EvidenceType::Sgx { new_pubkey } => keccak(
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
                    .copied()
                    .skip(32) // TRICKY: skip the first dyn flag 0x00..20.
                    .collect::<Vec<u8>>(),
            )
            .into(),
            EvidenceType::PseZk => todo!(),
            EvidenceType::Powdr => todo!(),
            EvidenceType::Succinct => keccak(
                (
                    self.transition.clone(),
                    // no pubkey since we don't need TEE to sign
                    self.prover,
                    self.meta_hash(),
                )
                    .abi_encode(),
            )
            .into(),
            EvidenceType::Risc0 | EvidenceType::Native => {
                keccak((self.transition.clone(), self.prover, self.meta_hash()).abi_encode()).into()
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EvidenceType {
    Sgx {
        new_pubkey: Address, // the evidence signature public key
    },
    PseZk,
    Powdr,
    Succinct,
    Risc0,
    Native,
}

pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;
pub fn kzg_to_versioned_hash(commitment: &KzgCommitment) -> B256 {
    let mut res = Sha256::digest(commitment.as_slice());
    res[0] = VERSIONED_HASH_VERSION_KZG;
    B256::new(res.into())
}

pub fn assemble_protocol_instance(
    input: &GuestInput,
    header: &AlloyConsensusHeader,
) -> Result<ProtocolInstance> {
    let blob_used = input.taiko.block_proposed.meta.blobUsed;
    let tx_list_hash = if blob_used {
        if input.taiko.skip_verify_blob {
            println!("kzg check disabled!");
            input.taiko.tx_blob_hash.unwrap()
        } else {
            println!("kzg check enabled!");
            let mut data = Vec::from(KZG_TRUST_SETUP_DATA);
            let kzg_settings = KzgSettings::from_u8_slice(&mut data);
            let kzg_commit = KzgCommitment::blob_to_kzg_commitment(
                &Blob::from_bytes(input.taiko.tx_data.as_slice()).unwrap(),
                &kzg_settings,
            )
            .unwrap();
            let versioned_hash = kzg_to_versioned_hash(&kzg_commit);
            assert_eq!(versioned_hash, input.taiko.tx_blob_hash.unwrap());
            versioned_hash
        }
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
        assert_eq!(
            input.chain_spec.max_spec_id, verified_chain_spec.max_spec_id,
            "unexpected max_spec_id"
        );
        assert_eq!(
            input.chain_spec.hard_forks, verified_chain_spec.hard_forks,
            "unexpected hard_forks"
        );
        assert_eq!(
            input.chain_spec.eip_1559_constants, verified_chain_spec.eip_1559_constants,
            "unexpected eip_1559_constants"
        );
        assert_eq!(
            input.chain_spec.l1_contract, verified_chain_spec.l1_contract,
            "unexpected l1_contract"
        );
        assert_eq!(
            input.chain_spec.l2_contract, verified_chain_spec.l2_contract,
            "unexpected l2_contract"
        );
        assert_eq!(
            input.chain_spec.is_taiko, verified_chain_spec.is_taiko,
            "unexpected eip_1559_constants"
        );
    }

    let deposits = input
        .withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect::<Vec<_>>();

    let gas_limit: u64 = header.gas_limit.try_into().unwrap();
    let pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            stateRoot: header.state_root,
            graffiti: input.taiko.prover_data.graffiti,
        },
        block_metadata: BlockMetadata {
            l1Hash: input.taiko.l1_header.hash(),
            difficulty: input.taiko.block_proposed.meta.difficulty,
            blobHash: tx_list_hash,
            extraData: bytes_to_bytes32(&header.extra_data).into(),
            depositsHash: keccak(deposits.abi_encode()).into(),
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (gas_limit
                - if input.chain_spec.is_taiko() {
                    ANCHOR_GAS_LIMIT
                } else {
                    0
                }) as u32,
            timestamp: header.timestamp,
            l1Height: input.taiko.l1_header.number,
            minTier: input.taiko.block_proposed.meta.minTier,
            blobUsed: blob_used,
            parentMetaHash: input.taiko.block_proposed.meta.parentMetaHash,
            sender: input.taiko.block_proposed.meta.sender,
        },
        prover: input.taiko.prover_data.prover,
        chain_id: input.chain_spec.chain_id,
        sgx_verifier_address: input.chain_spec.sgx_verifier_address.unwrap_or_default(),
    };

    // Sanity check
    if input.chain_spec.is_taiko() {
        ensure!(
            pi.block_metadata.abi_encode() == input.taiko.block_proposed.meta.abi_encode(),
            format!(
                "block hash mismatch, expected: {:?}, got: {:?}",
                input.taiko.block_proposed.meta, pi.block_metadata
            )
        );
    }

    Ok(pi)
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
    use raiko_primitives::keccak;

    use super::*;
    use crate::input::{proveBlockCall, TierProof};

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
        let pi_hash = keccak::keccak(
            (
                "VERIFY_PROOF",
                167001u64,
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
                trans.clone(),
                address!("741E45D08C70c1C232802711bBFe1B7C0E1acc55"),
                address!("70997970C51812dc3A010C7d01b50e0d17dc79C8"),
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
        let input = "10d008bd000000000000000000000000000000000000000000000000000000000000004900000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000340689c98d83627e8749504eb6effbc2b08408183f11211bbf8bd281727b16255e6b3f8ee61d80cd7d30cdde9aa49acac0b82264a6b0f992139398e95636e501fd80189249f72753bd6c715511cc61facdec4781d4ecb1d028dafdff4a0827d7d53302e31382e302d64657600000000000000000000000000000000000000000000569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd00000000000000000000000016700100000000000000000000000000000100010000000000000000000000000000000000000000000000000000000000000049000000000000000000000000000000000000000000000000000000000e4e1c000000000000000000000000000000000000000000000000000000000065f94010000000000000000000000000000000000000000000000000000000000000036000000000000000000000000000000000000000000000000000000000000000640000000000000000000000000000000000000000000000000000000000000001fdbdc45da60168ddf29b246eb9e0a2e612a670f671c6d3aafdfdac21f86b4bca0000000000000000000000003c44cdddb6a900fa2b585dd299e03d12fa4293bcaf73b06ee94a454236314610c55e053df3af4402081df52c9ff2692349a6b497bc17a6706bc1cf4c363e800d2133d0d143363871d9c17b8fc5cf6d3cfd585bc80730a40cf8d8186241d45e19785c117956de919999d50e473aaa794b8fd4097000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000260000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000064ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000";
        let input_data = hex::decode(input).unwrap();
        let proveBlockCall { blockId: _, input } =
            proveBlockCall::abi_decode(&input_data, false).unwrap();
        let (meta, trans, _proof) =
            <(BlockMetadata, Transition, TierProof)>::abi_decode_params(&input, false).unwrap();
        let meta_hash: B256 = keccak::keccak(meta.abi_encode()).into();
        let pi_hash = keccak::keccak(
            (
                "VERIFY_PROOF",
                10086u64,
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
                trans.clone(),
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
                address!("4F3F0D5B22338f1f991a1a9686C7171389C97Ff7"),
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
            "e9a8ebed81fb2da780c79aef3739c64c485373250b6167719517157936a1501b"
        );
    }
}
