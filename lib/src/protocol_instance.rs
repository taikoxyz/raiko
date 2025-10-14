use core::fmt::Display;

use alloy_primitives::{b256, Address, TxHash, B256};
use alloy_sol_types::SolValue;
use anyhow::{ensure, Result};
use pretty_assertions::Comparison;
use reth_evm_ethereum::taiko::decode_anchor_pacaya;
use reth_primitives::{Block, Header};

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{
    consts::SupportedChainSpecs,
    input::{
        ontake::{BlockMetadataV2, BlockProposedV2},
        pacaya::{BatchInfo, BatchMetadata, BlockParams, Transition as PacayaTransition},
        BlobProofType, BlockMetadata, BlockProposed, BlockProposedFork, GuestBatchInput,
        GuestInput, Transition,
    },
    primitives::{
        eip4844::{self, commitment_to_version_hash},
        keccak::keccak,
    },
    proof_type::ProofType,
    CycleTracker,
};
use reth_evm_ethereum::taiko::ANCHOR_GAS_LIMIT;
use tracing::{debug, info};

// The empty root of [`Vec<EthDeposit>`]
const EMPTY_ETH_DEPOSIT_ROOT: B256 =
    b256!("569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd");

#[derive(Debug, Clone)]
pub enum BlockMetaDataFork {
    None,
    Hekla(BlockMetadata),
    Ontake(BlockMetadataV2),
    Pacaya(BatchMetadata),
}

impl From<(&GuestInput, &Header, B256, &BlockProposed)> for BlockMetadata {
    fn from(
        (input, header, tx_list_hash, block_proposed): (&GuestInput, &Header, B256, &BlockProposed),
    ) -> Self {
        Self {
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (header.gas_limit
                - if input.chain_spec.is_taiko() {
                    ANCHOR_GAS_LIMIT
                } else {
                    0
                }) as u32,
            timestamp: header.timestamp,
            extraData: bytes_to_bytes32(&header.extra_data).into(),

            l1Hash: input.taiko.l1_header.hash_slow(),
            l1Height: input.taiko.l1_header.number,

            blobHash: tx_list_hash,

            depositsHash: EMPTY_ETH_DEPOSIT_ROOT,

            difficulty: block_proposed.meta.difficulty,
            minTier: block_proposed.meta.minTier,
            blobUsed: block_proposed.meta.blobUsed,
            parentMetaHash: block_proposed.meta.parentMetaHash,
            sender: block_proposed.meta.sender,
        }
    }
}

impl From<(&GuestInput, &Header, B256, &BlockProposedV2)> for BlockMetadataV2 {
    fn from(
        (input, header, tx_list_hash, block_proposed): (
            &GuestInput,
            &Header,
            B256,
            &BlockProposedV2,
        ),
    ) -> Self {
        Self {
            id: header.number,
            coinbase: header.beneficiary,
            timestamp: header.timestamp,
            gasLimit: (header.gas_limit
                - if input.chain_spec.is_taiko() {
                    ANCHOR_GAS_LIMIT
                } else {
                    0
                }) as u32,
            extraData: bytes_to_bytes32(&header.extra_data).into(),

            anchorBlockId: input.taiko.l1_header.number,
            anchorBlockHash: input.taiko.l1_header.hash_slow(),
            blobHash: tx_list_hash,

            difficulty: block_proposed.meta.difficulty,
            minTier: block_proposed.meta.minTier,
            blobUsed: block_proposed.meta.blobUsed,
            parentMetaHash: block_proposed.meta.parentMetaHash,
            proposer: block_proposed.meta.proposer,
            livenessBond: block_proposed.meta.livenessBond,
            proposedAt: block_proposed.meta.proposedAt,
            proposedIn: block_proposed.meta.proposedIn,
            blobTxListOffset: block_proposed.meta.blobTxListOffset,
            blobTxListLength: block_proposed.meta.blobTxListLength,
            blobIndex: block_proposed.meta.blobIndex,
            baseFeeConfig: block_proposed.meta.baseFeeConfig.clone(),
        }
    }
}

impl BlockMetaDataFork {
    fn from(input: &GuestInput, header: &Header, tx_list_hash: B256) -> Self {
        match &input.taiko.block_proposed {
            BlockProposedFork::Nothing => Self::None,
            BlockProposedFork::Hekla(block_proposed) => {
                Self::Hekla((input, header, tx_list_hash, block_proposed).into())
            }
            BlockProposedFork::Ontake(block_proposed_v2) => {
                Self::Ontake((input, header, tx_list_hash, block_proposed_v2).into())
            }
            BlockProposedFork::Pacaya(_batch_proposed) => {
                unimplemented!("single block signature is not supported for pacaya fork")
            }
        }
    }

    fn calculate_pacaya_txs_hash(tx_list_hash: B256, blob_hashes: &Vec<B256>) -> B256 {
        debug!(
            "calculate_pacaya_txs_hash from tx_list_hash: {:?}, blob_hashes: {:?}",
            tx_list_hash, blob_hashes
        );

        let abi_encode_data: Vec<u8> = (tx_list_hash, blob_hashes.iter().collect::<Vec<_>>())
            .abi_encode()
            .split_off(32);
        debug!("abi_encode_data: {:?}", hex::encode(&abi_encode_data));
        keccak(abi_encode_data).into()
    }

    fn from_batch_inputs(batch_input: &GuestBatchInput, final_blocks: Vec<Block>) -> Self {
        match &batch_input.taiko.batch_proposed {
            BlockProposedFork::Pacaya(batch_proposed) => {
                // todo: review the calculation 1 by 1 to make sure all of them are rooted from a trustable source
                let txs_hash = Self::calculate_pacaya_txs_hash(
                    keccak(batch_input.taiko.tx_data_from_calldata.as_slice()).into(),
                    &batch_proposed.info.blobHashes,
                );
                assert_eq!(
                    txs_hash, batch_proposed.info.txsHash,
                    "txs hash mismatch, expected: {:?}, got: {:?}",
                    txs_hash, batch_proposed.info.txsHash,
                );
                let ts_base = final_blocks.first().unwrap().timestamp;
                let (_, blocks) = final_blocks
                    .iter()
                    .zip(batch_proposed.info.blocks.iter())
                    .enumerate()
                    .fold(
                        (ts_base, Vec::new()),
                        |parent_ts_with_block_params, (index, (block, proposal_info))| {
                            let (parent_ts, mut block_params) = parent_ts_with_block_params;
                            let anchor_tx =
                                batch_input.inputs[index].taiko.anchor_tx.clone().unwrap();
                            let anchor_data = decode_anchor_pacaya(anchor_tx.input()).unwrap();
                            let signal_slots = anchor_data._signalSlots.clone();
                            assert!(
                                block.timestamp >= parent_ts
                                    && (block.timestamp - parent_ts) <= u8::MAX as u64
                            );
                            block_params.push(BlockParams {
                                numTransactions: proposal_info.numTransactions, // exclude anchor tx
                                timeShift: (block.timestamp - parent_ts) as u8,
                                signalSlots: signal_slots,
                            });
                            (block.timestamp, block_params)
                        },
                    );
                let blob_hashes = batch_proposed.info.blobHashes.clone();
                let extra_data = batch_proposed.info.extraData;
                let coinbase = batch_proposed.info.coinbase;
                let proposed_in = batch_proposed.info.proposedIn;
                let blob_created_in = batch_proposed.info.blobCreatedIn;
                let blob_byte_offset = batch_proposed.info.blobByteOffset;
                let blob_byte_size = batch_proposed.info.blobByteSize;
                let gas_limit = batch_proposed.info.gasLimit;
                let last_block_id = final_blocks.last().unwrap().header.number;
                assert!(
                    last_block_id == batch_proposed.info.lastBlockId,
                    "last block id mismatch, expected: {:?}, got: {:?}",
                    last_block_id,
                    batch_proposed.info.lastBlockId,
                );
                let last_block_timestamp = final_blocks.last().unwrap().header.timestamp;
                assert!(
                    last_block_timestamp == batch_proposed.info.lastBlockTimestamp,
                    "last block timestamp mismatch, expected: {:?}, got: {:?}",
                    last_block_timestamp,
                    batch_proposed.info.lastBlockTimestamp,
                );
                // checked in anchor_check()
                let anchor_block_id = batch_input.taiko.l1_header.number;
                let anchor_block_hash = batch_input.taiko.l1_header.hash_slow();
                let base_fee_config = batch_proposed.info.baseFeeConfig.clone();
                BlockMetaDataFork::Pacaya(BatchMetadata {
                    // todo: keccak data based on input
                    infoHash: keccak(
                        BatchInfo {
                            txsHash: txs_hash,
                            blocks,
                            blobHashes: blob_hashes,
                            extraData: extra_data,
                            coinbase,
                            proposedIn: proposed_in,
                            blobCreatedIn: blob_created_in,
                            blobByteOffset: blob_byte_offset,
                            blobByteSize: blob_byte_size,
                            gasLimit: gas_limit,
                            lastBlockId: last_block_id,
                            lastBlockTimestamp: last_block_timestamp,
                            anchorBlockId: anchor_block_id,
                            anchorBlockHash: anchor_block_hash,
                            baseFeeConfig: base_fee_config,
                        }
                        .abi_encode(),
                    )
                    .into(),
                    proposer: batch_proposed.meta.proposer,
                    batchId: batch_input.taiko.batch_id,
                    proposedAt: batch_proposed.meta.proposedAt,
                })
            }
            _ => {
                unimplemented!("batch blocks signature is not supported before pacaya fork")
            }
        }
    }

    fn match_block_proposal<'a>(
        &'a self,
        other: &'a BlockProposedFork,
    ) -> (bool, Option<Box<dyn Display + 'a>>) {
        match (self, other) {
            (Self::Hekla(a), BlockProposedFork::Hekla(b)) => (
                a.abi_encode() == b.meta.abi_encode(),
                Some(Box::new(Comparison::new(a, &b.meta))),
            ),
            (Self::Ontake(a), BlockProposedFork::Ontake(b)) => (
                a.abi_encode() == b.meta.abi_encode(),
                Some(Box::new(Comparison::new(a, &b.meta))),
            ),
            (Self::Pacaya(a), BlockProposedFork::Pacaya(b)) => (
                a.abi_encode() == b.meta.abi_encode(),
                Some(Box::new(Comparison::new(a, &b.meta))),
            ),
            (Self::None, BlockProposedFork::Nothing) => (true, None),
            _ => (false, None),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TransitionFork {
    Hekla(Transition),
    OnTake(Transition),
    Pacaya(PacayaTransition),
}

#[derive(Debug, Clone)]
pub struct ProtocolInstance {
    pub transition: TransitionFork,
    pub block_metadata: BlockMetaDataFork,
    pub prover: Address,
    pub sgx_instance: Address, // only used for SGX
    pub chain_id: u64,
    pub verifier_address: Address,
}

fn verify_blob(
    blob_proof_type: BlobProofType,
    blob_data: &[u8],
    versioned_hash: B256,
    commitment: &[u8; 48],
    blob_proof: Option<Vec<u8>>,
) -> Result<()> {
    info!("blob proof type: {:?}", &blob_proof_type);
    match blob_proof_type {
        crate::input::BlobProofType::ProofOfEquivalence => {
            let ct = CycleTracker::start("proof_of_equivalence");
            let (x, y) = eip4844::proof_of_equivalence(blob_data, &versioned_hash)?;
            ct.end();
            let verified = eip4844::verify_kzg_proof_impl(
                *commitment,
                x,
                y,
                blob_proof
                    .map(|p| TryInto::<[u8; 48]>::try_into(p).unwrap())
                    .unwrap(),
            )?;
            ensure!(verified);
        }
        BlobProofType::KzgVersionedHash => {
            let ct = CycleTracker::start("proof_of_commitment");
            ensure!(commitment == &eip4844::calc_kzg_proof_commitment(blob_data)?);
            ct.end();
        }
    };
    Ok(())
}

/// Verify the blob usage in batch mode, i.e., check if raw blob commitment == input blob commitment
/// then the blob version hash is calculated from the blob data, and eventually get connected to the
/// on-chain blob hash.
fn verify_batch_mode_blob_usage(
    batch_input: &GuestBatchInput,
    proof_type: ProofType,
) -> Result<()> {
    let blob_proof_type =
        get_blob_proof_type(proof_type, batch_input.taiko.blob_proof_type.clone());

    match blob_proof_type {
        crate::input::BlobProofType::KzgVersionedHash => assert_eq!(
            batch_input.taiko.tx_data_from_blob.len(),
            batch_input
                .taiko
                .blob_commitments
                .as_ref()
                .map_or(0, |c| c.len()),
            "Each blob should have its own hash commit"
        ),
        crate::input::BlobProofType::ProofOfEquivalence => assert_eq!(
            batch_input.taiko.tx_data_from_blob.len(),
            batch_input
                .taiko
                .blob_proofs
                .as_ref()
                .map_or(0, |p| p.len()),
            "Each blob should have its own proof"
        ),
    }

    for blob_verify_param in batch_input
        .taiko
        .tx_data_from_blob
        .iter()
        .zip(
            batch_input
                .taiko
                .blob_commitments
                .clone()
                .unwrap_or_default()
                .iter(),
        )
        .zip(
            batch_input
                .taiko
                .blob_proofs
                .clone()
                .unwrap_or_default()
                .iter(),
        )
    {
        let blob_data = blob_verify_param.0 .0;
        let commitment = blob_verify_param.0 .1;
        let versioned_hash = commitment_to_version_hash(&commitment.clone().try_into().unwrap());
        debug!(
            "verify_batch_mode_blob_usage commitment: {:?}, hash: {:?}",
            hex::encode(commitment),
            versioned_hash
        );
        verify_blob(
            blob_proof_type.clone(),
            blob_data,
            versioned_hash,
            &commitment.clone().try_into().unwrap(),
            Some(blob_verify_param.1.clone()),
        )?;
    }
    Ok(())
}

impl ProtocolInstance {
    pub fn new(input: &GuestInput, header: &Header, proof_type: ProofType) -> Result<Self> {
        let blob_used = input.taiko.block_proposed.blob_used();
        // If blob is used, tx_list_hash is the commitment to the blob
        // and we need to verify the blob hash matches the blob data.
        // If we need to compute the proof of equivalence this data will be set.
        // Otherwise the proof_of_equivalence is 0
        let tx_list_hash = if blob_used {
            let commitment = input
                .taiko
                .blob_commitment
                .as_ref()
                .expect("no blob commitment");
            let versioned_hash =
                commitment_to_version_hash(&commitment.clone().try_into().unwrap());

            verify_blob(
                get_blob_proof_type(proof_type, input.taiko.blob_proof_type.clone()),
                &input.taiko.tx_data,
                versioned_hash,
                &commitment.clone().try_into().unwrap(),
                input.taiko.blob_proof.clone(),
            )?;
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
            .get_fork_verifier_address(input.taiko.block_proposed.block_number(), proof_type)
            .unwrap_or_default();

        let transition = match input.taiko.block_proposed {
            BlockProposedFork::Hekla(_) => TransitionFork::Hekla(Transition {
                parentHash: header.parent_hash,
                blockHash: header.hash_slow(),
                stateRoot: header.state_root,
                graffiti: input.taiko.prover_data.graffiti,
            }),
            BlockProposedFork::Ontake(_) => TransitionFork::OnTake(Transition {
                parentHash: header.parent_hash,
                blockHash: header.hash_slow(),
                stateRoot: header.state_root,
                graffiti: input.taiko.prover_data.graffiti,
            }),
            _ => return Err(anyhow::Error::msg("unknown transition fork")),
        };

        let pi = ProtocolInstance {
            transition,
            block_metadata: BlockMetaDataFork::from(input, header, tx_list_hash),
            sgx_instance: Address::default(),
            prover: input.taiko.prover_data.prover,
            chain_id: input.chain_spec.chain_id,
            verifier_address,
        };

        // Sanity check
        if input.chain_spec.is_taiko() {
            let (same, pretty_display) = pi
                .block_metadata
                .match_block_proposal(&input.taiko.block_proposed);
            ensure!(
                same,
                format!("block hash mismatch: {}", pretty_display.unwrap(),)
            );
        }

        Ok(pi)
    }

    pub fn new_batch(
        batch_input: &GuestBatchInput,
        blocks: Vec<Block>,
        proof_type: ProofType,
    ) -> Result<Self> {
        // verify blob usage, either by commitment or proof equality.
        verify_batch_mode_blob_usage(batch_input, proof_type)?;

        for input in &batch_input.inputs {
            // If the passed in chain spec contains a known chain id, the chain spec NEEDS to match the
            // one we expect, because the prover could otherwise just fill in any values.
            // The chain id is used because that is the value that is put onchain,
            // and so all other chain data needs to be derived from it.
            // For unknown chain ids we just skip this check so that tests using test data can still pass.
            // TODO: we should probably split things up in critical and non-critical parts
            // in the chain spec itself so we don't have to manually all the ones we have to care about.
            if let Some(verified_chain_spec) = SupportedChainSpecs::default()
                .get_chain_spec_with_chain_id(input.chain_spec.chain_id)
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
        }

        // todo: move chain_spec into the batch input
        let input = &batch_input.inputs[0];
        let verifier_address = input
            .chain_spec
            .get_fork_verifier_address(input.taiko.block_proposed.block_number(), proof_type)
            .unwrap_or_default();

        let first_block = blocks.first().unwrap();
        let last_block = blocks.last().unwrap();
        let transition = match batch_input.taiko.batch_proposed {
            BlockProposedFork::Pacaya(_) => TransitionFork::Pacaya(PacayaTransition {
                parentHash: first_block.header.parent_hash,
                blockHash: last_block.header.hash_slow(),
                stateRoot: last_block.header.state_root,
            }),
            _ => return Err(anyhow::Error::msg("unknown transition fork")),
        };

        let pi = ProtocolInstance {
            transition,
            block_metadata: BlockMetaDataFork::from_batch_inputs(batch_input, blocks),
            sgx_instance: Address::default(),
            prover: input.taiko.prover_data.prover,
            chain_id: input.chain_spec.chain_id,
            verifier_address,
        };

        // Sanity check
        if input.chain_spec.is_taiko() {
            let (same, pretty_display) = pi
                .block_metadata
                .match_block_proposal(&batch_input.taiko.batch_proposed);
            ensure!(
                same,
                format!("batch block hash mismatch: {}", pretty_display.unwrap(),)
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
            BlockMetaDataFork::None => keccak(vec![]).into(),
            BlockMetaDataFork::Hekla(ref meta) => keccak(meta.abi_encode()).into(),
            BlockMetaDataFork::Ontake(ref meta) => keccak(meta.abi_encode()).into(),
            BlockMetaDataFork::Pacaya(ref meta) => keccak(meta.abi_encode()).into(),
        }
    }

    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn instance_hash(&self) -> B256 {
        // packages/protocol/contracts/verifiers/libs/LibPublicInput.sol
        // "VERIFY_PROOF", _chainId, _verifierContract, _tran, _newInstance, _prover, _metaHash
        debug!(
            "calculate instance_hash from:
            chain_id: {:?}, verifier: {:?}, transition: {:?}, sgx_instance: {:?},
            prover: {:?}, block_meta: {:?}, meta_hash: {:?}",
            self.chain_id,
            self.verifier_address,
            &self.transition,
            self.sgx_instance,
            self.prover,
            &self.block_metadata,
            self.meta_hash(),
        );

        let data = match &self.transition {
            TransitionFork::Hekla(transition) | TransitionFork::OnTake(transition) => (
                "VERIFY_PROOF",
                self.chain_id,
                self.verifier_address,
                transition.clone(),
                self.sgx_instance,
                self.prover,
                self.meta_hash(),
            )
                .abi_encode()
                .iter()
                .skip(32)
                .copied()
                .collect::<Vec<u8>>(),
            TransitionFork::Pacaya(pacaya_trans) => (
                "VERIFY_PROOF",
                self.chain_id,
                self.verifier_address,
                pacaya_trans.clone(),
                self.sgx_instance,
                self.meta_hash(),
            )
                .abi_encode()
                .iter()
                .skip(32)
                .copied()
                .collect::<Vec<u8>>(),
        };
        keccak(data).into()
    }
}

// Make sure the verifier supports the blob proof type
fn get_blob_proof_type(
    proof_type: ProofType,
    blob_proof_type_hint: BlobProofType,
) -> BlobProofType {
    // Enforce different blob proof type for different provers
    // due to performance considerations
    match proof_type {
        ProofType::Native => blob_proof_type_hint,
        ProofType::Sgx | ProofType::SgxGeth => BlobProofType::KzgVersionedHash,
        ProofType::Sp1 | ProofType::Risc0 | ProofType::OpenVM => BlobProofType::ProofOfEquivalence,
    }
}

fn bytes_to_bytes32(input: &[u8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    let len = core::cmp::min(input.len(), 32);
    bytes[..len].copy_from_slice(&input[..len]);
    bytes
}

pub fn words_to_bytes_le(words: &[u32; 8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for i in 0..8 {
        let word_bytes = words[i].to_le_bytes();
        bytes[i * 4..(i + 1) * 4].copy_from_slice(&word_bytes);
    }
    bytes
}

pub fn words_to_bytes_be(words: &[u32; 8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for i in 0..8 {
        let word_bytes = words[i].to_be_bytes();
        bytes[i * 4..(i + 1) * 4].copy_from_slice(&word_bytes);
    }
    bytes
}

pub fn aggregation_output_combine(public_inputs: Vec<B256>) -> Vec<u8> {
    let mut output = Vec::with_capacity(public_inputs.len() * 32);
    for public_input in public_inputs.iter() {
        output.extend_from_slice(&public_input.0);
    }
    output
}

pub fn aggregation_output(program: B256, public_inputs: Vec<B256>) -> Vec<u8> {
    aggregation_output_combine([vec![program], public_inputs].concat())
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
        let trans = PacayaTransition {
            parentHash: b256!("0b8cbe268ead34bf55d650ea6e456bf50ed0fe16324b3570c01233735fd58add"),
            blockHash: b256!("77292283b80b9082ae261ce71a2997b1e7d1f50b9d0c992aeeca4017a7363075"),
            stateRoot: b256!("f57a1a8ed47fab33d3dc754d116993fcdf2530299c9ad2f8424e01b4b02637b9"),
        };

        let meta_hash = b256!("27b5964fa27493b7ecd120f8bae4280b4616a1fd8fe200d219c41178e29b28fa");
        let pi_hash = keccak::keccak(
            (
                "VERIFY_PROOF",
                167001u64,
                address!("0Cf58F3E8514d993cAC87Ca8FC142b83575cC4D3"),
                trans.clone(),
                address!("5EA7F24Afb55295586aCeFCeA81d48A4C3F543fa"),
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
            "eec345cd2a4dce74689e5e8b021f142a9d51d2791d7e06d130ae8ac14676f1df"
        );
    }

    #[test]
    fn test_aggregation_pi() {
        let old_instance =
            Address::from_slice(&hex::decode("5EA7F24Afb55295586aCeFCeA81d48A4C3F543fa").unwrap());
        let agg_pi = keccak::keccak(aggregation_output_combine(
            [
                vec![
                    B256::left_padding_from(old_instance.as_ref()),
                    B256::left_padding_from(old_instance.as_ref()),
                ],
                vec![b256!(
                    "eec345cd2a4dce74689e5e8b021f142a9d51d2791d7e06d130ae8ac14676f1df"
                )],
            ]
            .concat(),
        ));
        // println!("agg_pi = {:?}", hex::encode(agg_pi));
        assert_eq!(
            hex::encode(agg_pi),
            "57c0a252e84a6f366a8124a45dde00ffc2a28875d1d347fba9835d316e20b74a"
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
