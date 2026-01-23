use core::fmt::Display;
use std::collections::HashSet;

use alloy_primitives::{b256, Address, TxHash, B256};
use alloy_sol_types::SolValue;
use anyhow::{ensure, Ok, Result};
use pretty_assertions::Comparison;
use reth_evm_ethereum::taiko::{decode_anchor_pacaya, decode_anchor_shasta};
use reth_primitives::{Block, Header};

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{
    consts::SupportedChainSpecs,
    input::{
        ontake::{BlockMetadataV2, BlockProposedV2},
        pacaya::{BatchInfo, BatchMetadata, BlockParams, Transition as PacayaTransition},
        shasta::{Checkpoint, Commitment, Proposal as ShastaProposal},
        BlobProofType, BlockMetadata, BlockProposed, BlockProposedFork, GuestBatchInput,
        GuestInput, ShastaRawAggregationGuestInput, Transition,
    },
    libhash::{
        hash_commitment, hash_proposal, hash_public_input, hash_shasta_subproof_input,
        hash_two_values,
    },
    primitives::{
        eip4844::{self, commitment_to_version_hash},
        keccak::keccak,
    },
    proof_type::ProofType,
    prover::{ProofCarryData, ShastaTransitionInput, TransitionInputData},
    CycleTracker,
};
use reth_evm_ethereum::taiko::ANCHOR_GAS_LIMIT;
use tracing::{debug, error, info};

// The empty root of [`Vec<EthDeposit>`]
const EMPTY_ETH_DEPOSIT_ROOT: B256 =
    b256!("569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd");

#[derive(Debug, Clone)]
pub enum BlockMetaDataFork {
    None,
    Hekla(BlockMetadata),
    Ontake(BlockMetadataV2),
    Pacaya(BatchMetadata),
    Shasta(ShastaProposal),
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
            BlockProposedFork::Shasta(_) => {
                unimplemented!("single block signature is not supported for shasta fork")
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
                    keccak(
                        batch_input.taiko.data_sources[0]
                            .tx_data_from_calldata
                            .as_slice(),
                    )
                    .into(),
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
            BlockProposedFork::Shasta(event_data) => {
                // TODO(shasta): add a full Shasta block metadata reconstruction & comparison,
                // similar to Pacaya's infoHash / txsHash binding.
                BlockMetaDataFork::Shasta(event_data.proposal.clone())
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
            (Self::Shasta(_a), BlockProposedFork::Shasta(_b)) => {
                //todo: match shasta proposal
                (true, None)
            }
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
    Shasta(TransitionInputData),
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
    expected_versioned_hash: &B256,
    commitment: &[u8; 48],
    blob_proof: Option<Vec<u8>>,
) -> Result<()> {
    info!("blob proof type: {:?}", &blob_proof_type);
    match blob_proof_type {
        crate::input::BlobProofType::ProofOfEquivalence => {
            // Even in PoE mode, the blob must be anchored to the on-chain versioned hash.
            ensure!(*expected_versioned_hash == commitment_to_version_hash(commitment));

            let ct = CycleTracker::start("proof_of_equivalence");
            let (x, y) = eip4844::proof_of_equivalence(blob_data, &expected_versioned_hash)?;
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
            ensure!(
                *expected_versioned_hash
                    == commitment_to_version_hash(&commitment.clone().try_into().unwrap())
            );
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
    // Expected on-chain blob hashes for each data source (Shasta: one per DerivationSource,
    // Pacaya: a single list for the batch).
    let expected_blob_hashes_per_source: Vec<Vec<B256>> =
        batch_input.taiko.batch_proposed.all_source_blob_hashes();
    ensure!(
        expected_blob_hashes_per_source.len() == batch_input.taiko.data_sources.len(),
        "data_sources length mismatch: expected {}, got {}",
        expected_blob_hashes_per_source.len(),
        batch_input.taiko.data_sources.len()
    );

    for (source_idx, data_source) in batch_input.taiko.data_sources.iter().enumerate() {
        let blob_proof_type = get_blob_proof_type(proof_type, data_source.blob_proof_type.clone());
        let source_blob_hashes =
            expected_blob_hashes_per_source
                .get(source_idx)
                .ok_or_else(|| {
                    anyhow::anyhow!("missing expected blob hashes for source {source_idx}")
                })?;
        ensure!(
            source_blob_hashes.len() == data_source.tx_data_from_blob.len(),
            "source blob hashes length mismatch at source {}: expected {}, got {}",
            source_idx,
            source_blob_hashes.len(),
            data_source.tx_data_from_blob.len()
        );
        assert_eq!(
            data_source.tx_data_from_blob.len(),
            data_source.blob_commitments.as_ref().map_or(0, |c| c.len()),
            "Each blob should have its own hash commit"
        );
        match blob_proof_type {
            crate::input::BlobProofType::KzgVersionedHash => {
                let commitments = data_source
                    .blob_commitments
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing blob commitments"))?;
                for blob_verify_param in data_source
                    .tx_data_from_blob
                    .iter()
                    .zip(commitments.iter())
                    .zip(source_blob_hashes.iter())
                {
                    let blob_data = blob_verify_param.0 .0;
                    let commitment = blob_verify_param.0 .1;
                    let expected_blob_hash = blob_verify_param.1;
                    verify_blob(
                        blob_proof_type.clone(),
                        blob_data,
                        expected_blob_hash,
                        &commitment
                            .as_slice()
                            .try_into()
                            .map_err(|_| anyhow::anyhow!("invalid blob commitment length"))?,
                        None,
                    )?;
                }
            }
            crate::input::BlobProofType::ProofOfEquivalence => {
                assert_eq!(
                    data_source.tx_data_from_blob.len(),
                    data_source.blob_proofs.as_ref().map_or(0, |p| p.len()),
                    "Each blob should have its own proof in PoE mode"
                );
                let proofs = data_source
                    .blob_proofs
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing blob proofs"))?;
                let commitments = data_source
                    .blob_commitments
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing blob commitments"))?;
                for blob_verify_param in data_source
                    .tx_data_from_blob
                    .iter()
                    .zip(commitments.iter())
                    .zip(proofs.iter())
                    .zip(source_blob_hashes.iter())
                {
                    let blob_data = blob_verify_param.0 .0 .0;
                    let commitment = blob_verify_param.0 .0 .1;
                    let proof = blob_verify_param.0 .1;
                    let expected_blob_hash = blob_verify_param.1;
                    verify_blob(
                        blob_proof_type.clone(),
                        blob_data,
                        expected_blob_hash,
                        &commitment
                            .as_slice()
                            .try_into()
                            .map_err(|_| anyhow::anyhow!("invalid blob commitment length"))?,
                        Some(proof.clone()),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn verify_shasha_anchor_linkage(
    inputs: &[GuestInput],
    l1_ancestor_headers: &[Header],
    expected_parent_hash: &B256,
) -> bool {
    let mut anchor_param_set = HashSet::new();
    for input in inputs {
        let anchor_tx = input.taiko.anchor_tx.clone().unwrap();
        let anchor_data = decode_anchor_shasta(anchor_tx.input()).unwrap();
        anchor_param_set.insert((
            anchor_data._checkpoint.blockNumber,
            anchor_data._checkpoint.blockHash,
            anchor_data._checkpoint.stateRoot,
        ));
    }

    if l1_ancestor_headers.is_empty() {
        error!("l1 ancestor headers is empty");
        return false;
    }

    let mut last_parent_hash = l1_ancestor_headers[0].hash_slow();
    let mut l1_ancestor_hash_set = HashSet::from([(
        l1_ancestor_headers[0].number,
        last_parent_hash,
        l1_ancestor_headers[0].state_root,
    )]);
    for curr in l1_ancestor_headers.iter().skip(1) {
        if curr.parent_hash != last_parent_hash {
            error!(
                "l1 ancestor header parent hash mismatch, expected: {:?}, got: {:?}",
                last_parent_hash, curr.parent_hash
            );
            return false;
        }
        let curr_hash = curr.hash_slow();
        l1_ancestor_hash_set.insert((curr.number, curr_hash, curr.state_root));
        last_parent_hash = curr_hash;
    }

    // every elem in anchor_param_set should be in l1_ancestor_hash_set
    for anchor_param in anchor_param_set {
        if !l1_ancestor_hash_set.contains(&(anchor_param.0, anchor_param.1, anchor_param.2)) {
            error!(
                "anchor param not found in l1 ancestor hash set: {:?}",
                anchor_param
            );
            return false;
        }
    }

    last_parent_hash == *expected_parent_hash
}

fn bypass_shasta_anchor_linkage(batch_input: &GuestBatchInput) -> bool {
    if !batch_input.taiko.l1_ancestor_headers.is_empty() {
        return false;
    }
    let mut anchors = batch_input
        .inputs
        .iter()
        .filter_map(|input| {
            input
                .taiko
                .anchor_tx
                .as_ref()
                .and_then(|tx| decode_anchor_shasta(tx.input()).ok())
                .map(|data| data._checkpoint.blockNumber)
        });
    let Some(first_anchor) = anchors.next() else {
        return false;
    };
    anchors.all(|h| h == first_anchor)
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
                &versioned_hash,
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
            .get_fork_verifier_address(
                input.taiko.block_proposed.block_number(),
                header.timestamp,
                proof_type,
            )
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
            prover: input.taiko.prover_data.actual_prover,
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
        let first_block: &Block = blocks.first().unwrap();
        let verifier_address = input
            .chain_spec
            .get_fork_verifier_address(input.block.number, first_block.header.timestamp, proof_type)
            .unwrap_or_default();

        let last_block = blocks.last().unwrap();
        let transition = match &batch_input.taiko.batch_proposed {
            BlockProposedFork::Pacaya(_) => TransitionFork::Pacaya(PacayaTransition {
                parentHash: first_block.header.parent_hash,
                blockHash: last_block.header.hash_slow(),
                stateRoot: last_block.header.state_root,
            }),
            BlockProposedFork::Shasta(event_data) => {
                if bypass_shasta_anchor_linkage(batch_input) {
                    info!("skip shasta anchor linkage verification due to stalled anchor");
                } else {
                    assert!(
                        verify_shasha_anchor_linkage(
                            &batch_input.inputs,
                            batch_input.taiko.l1_ancestor_headers.as_slice(),
                            &event_data.proposal.originBlockHash
                        ),
                        "L1 anchor linkage verification failed"
                    );
                }
                assert_eq!(
                    &event_data.proposal.originBlockNumber, &batch_input.taiko.l1_header.number,
                    "L1 origin block number mismatch"
                );
                assert_eq!(
                    event_data.proposal.originBlockHash,
                    batch_input.taiko.l1_header.hash_slow(),
                    "L1 origin block hash mismatch"
                );
                // check local re-constructed proposal matches the event proposal
                let last_block_number = last_block.number;
                let last_block_hash = last_block.header.hash_slow();
                let last_block_state_root = last_block.header.state_root;
                let current_transition_checkpoint = Checkpoint {
                    blockNumber: last_block_number,
                    blockHash: last_block_hash,
                    stateRoot: last_block_state_root,
                };
                if let Some(ref_checkpoint) = &batch_input.taiko.prover_data.checkpoint {
                    assert_eq!(
                        current_transition_checkpoint, *ref_checkpoint,
                        "checkpoint last block number mismatch, expected: {:?}, got: {:?}",
                        current_transition_checkpoint, ref_checkpoint
                    );
                }
                TransitionFork::Shasta(TransitionInputData {
                    proposal_id: event_data.proposal.id,
                    proposal_hash: hash_proposal(&event_data.proposal),
                    parent_proposal_hash: event_data.proposal.parentProposalHash,
                    parent_block_hash: batch_input.inputs[0].parent_header.hash_slow(),
                    actual_prover: batch_input.taiko.prover_data.actual_prover,
                    transition: ShastaTransitionInput {
                        proposer: event_data.proposal.proposer,
                        timestamp: event_data.proposal.timestamp,
                    },
                    checkpoint: current_transition_checkpoint,
                })
            }
            _ => return Err(anyhow::Error::msg("unknown transition fork")),
        };

        let pi = ProtocolInstance {
            transition,
            block_metadata: BlockMetaDataFork::from_batch_inputs(batch_input, blocks),
            sgx_instance: Address::default(),
            prover: batch_input.taiko.prover_data.actual_prover,
            chain_id: batch_input.taiko.chain_spec.chain_id,
            verifier_address,
        };

        // Sanity check
        if batch_input.taiko.chain_spec.is_taiko() {
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
            BlockMetaDataFork::Shasta(ref meta) => keccak(meta.abi_encode()).into(),
        }
    }

    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn instance_hash(&self) -> B256 {
        // packages/protocol/contracts/verifiers/libs/LibPublicInput.sol
        // "VERIFY_PROOF", _chainId, _verifierContract, _tran, _newInstance, _prover, _metaHash
        info!(
            "calculate instance_hash from:
            chain_id: {:?}, verifier: {:?}, transition: {:?}, sgx_instance: {:?},
            prover: {:?}, block_meta: {:?}, meta_hash: {:?}",
            self.chain_id,
            self.verifier_address,
            &self.transition,
            &self.sgx_instance,
            &self.prover,
            &self.block_metadata,
            self.meta_hash(),
        );

        match &self.transition {
            TransitionFork::Hekla(transition) | TransitionFork::OnTake(transition) => {
                let data = (
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
                    .collect::<Vec<u8>>();
                keccak(data).into()
            }
            TransitionFork::Pacaya(pacaya_trans) => {
                let data = (
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
                    .collect::<Vec<u8>>();
                keccak(data).into()
            }
            TransitionFork::Shasta(shasta_trans_input) => {
                info!(
                    "transition to be signed into public: {:?}.",
                    shasta_trans_input
                );
                // Domain separation: bind chain_id + verifier into the signed message.
                hash_shasta_subproof_input(&ProofCarryData {
                    chain_id: self.chain_id,
                    verifier: self.verifier_address,
                    transition_input: shasta_trans_input.clone(),
                })
            }
        }
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
        ProofType::Sp1 | ProofType::Risc0 => BlobProofType::ProofOfEquivalence,
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

pub fn validate_shasta_aggregate_proof_carry_data(
    aggregation_input: &ShastaRawAggregationGuestInput,
) -> bool {
    // The carry vector is meant to be a per-proof sidecar; treat mismatched sizes as invalid.
    if aggregation_input.proofs.len() != aggregation_input.proof_carry_data_vec.len() {
        return false;
    }
    validate_shasta_proof_carry_data_vec(&aggregation_input.proof_carry_data_vec)
}

pub fn validate_shasta_proof_carry_data_vec(proof_carry_data_vec: &[ProofCarryData]) -> bool {
    if proof_carry_data_vec.is_empty() {
        return false;
    }

    let expected_actual_prover = proof_carry_data_vec[0].transition_input.actual_prover;
    for item in proof_carry_data_vec.iter() {
        // Commitment uses a single `actualProver` field; make the range unambiguous.
        if item.transition_input.actual_prover != expected_actual_prover {
            return false;
        }
    }

    for w in proof_carry_data_vec.windows(2) {
        let prev = &w[0];
        let next = &w[1];
        // Ensure proposal ids are sequential
        if prev.transition_input.proposal_id + 1 != next.transition_input.proposal_id {
            return false;
        }

        // Ensure proposal hashes chain correctly
        if prev.transition_input.proposal_hash != next.transition_input.parent_proposal_hash {
            return false;
        }

        if prev.chain_id != next.chain_id {
            return false;
        }

        if prev.verifier != next.verifier {
            return false;
        }

        // Continuity: prev checkpoint must match next parent checkpoint hash.
        if prev.transition_input.checkpoint.blockHash != next.transition_input.parent_block_hash {
            return false;
        }
    }

    true
}

pub fn build_shasta_commitment_from_proof_carry_data_vec(
    proof_carry_data_vec: &[ProofCarryData],
) -> Option<Commitment> {
    if !validate_shasta_proof_carry_data_vec(proof_carry_data_vec) {
        return None;
    }
    let last = proof_carry_data_vec.last()?;

    let transitions: Vec<crate::input::shasta::Transition> = proof_carry_data_vec
        .iter()
        .map(|item| crate::input::shasta::Transition {
            proposer: item.transition_input.transition.proposer,
            timestamp: item.transition_input.transition.timestamp,
            blockHash: item.transition_input.checkpoint.blockHash,
        })
        .collect();

    Some(Commitment {
        firstProposalId: proof_carry_data_vec[0].transition_input.proposal_id,
        // This field is a checkpoint hash in the latest Shasta contract; we store it as bytes32.
        firstProposalParentBlockHash: proof_carry_data_vec[0].transition_input.parent_block_hash,
        lastProposalHash: last.transition_input.proposal_hash,
        actualProver: proof_carry_data_vec[0].transition_input.actual_prover,
        endBlockNumber: last.transition_input.checkpoint.blockNumber,
        endStateRoot: last.transition_input.checkpoint.stateRoot,
        transitions,
    })
}

fn shasta_aggregation_commitment_hash(
    prove_input: &Commitment,
    chain_id: u64,
    verifier_address: Address,
    sgx_instance: Address,
) -> B256 {
    let prove_input_hash = hash_commitment(&prove_input);
    hash_public_input(prove_input_hash, chain_id, verifier_address, sgx_instance)
}

pub fn shasta_pcd_aggregation_hash(
    proof_carry_data_vec: &[ProofCarryData],
    sgx_instance: Address,
) -> Option<B256> {
    let commitment = build_shasta_commitment_from_proof_carry_data_vec(proof_carry_data_vec)?;
    let first = proof_carry_data_vec.first()?;
    let aggregation_hash = shasta_aggregation_commitment_hash(
        &commitment,
        first.chain_id,
        first.verifier,
        sgx_instance,
    );
    Some(aggregation_hash)
}

pub fn shasta_aggregation_hash_for_zk(
    sub_image_id: B256,
    proof_carry_data_vec: &[ProofCarryData],
) -> Option<B256> {
    shasta_pcd_aggregation_hash(proof_carry_data_vec, Address::ZERO).map(|aggregation_hash| {
        bind_aggregate_hash_with_zk_image_id(sub_image_id, aggregation_hash)
    })
}

/// only for zk, as tee does not have sub image id so far.
fn bind_aggregate_hash_with_zk_image_id(sub_image_id: B256, sub_input_hash: B256) -> B256 {
    hash_two_values(sub_image_id, sub_input_hash)
}
#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};
    use alloy_sol_types::SolCall;

    use super::*;
    use crate::{
        input::{proveBlockCall, shasta::Checkpoint, TierProof},
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

    #[test]
    fn test_shasta_aggregation_output() {
        let chain_id = 167001u64;
        let verifier_address = address!("00f9f60C79e38c08b785eE4F1a849900693C6630");
        let sgx_instance = address!("dc95623058E847fA38e56a0Fa466Bf52C48eFA32");
        let prove_input = Commitment {
            firstProposalParentBlockHash: b256!(
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            lastProposalHash: b256!(
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            endBlockNumber: 1,
            endStateRoot: b256!("0000000000000000000000000000000000000000000000000000000000000000"),
            firstProposalId: 12345,
            actualProver: address!("1111111111111111111111111111111111111111"),
            transitions: vec![],
        };
        let result = shasta_aggregation_commitment_hash(
            &prove_input,
            chain_id,
            verifier_address,
            sgx_instance,
        );

        assert_eq!(
            result,
            b256!("5ffd635c42c7e6f7a5aa6c83be7db37dd1c24f1b474606ef0901b9b32beffaae")
        )
    }

    #[test]
    fn test_validate_shasta_aggregate_proof_carry_data_basic() {
        use crate::{
            input::{RawProof, ShastaRawAggregationGuestInput},
            prover::{ProofCarryData, TransitionInputData},
        };

        let chain_id = 167001u64;
        let verifier = address!("00f9f60C79e38c08b785eE4F1a849900693C6630");

        let p0_hash = b256!("1111111111111111111111111111111111111111111111111111111111111111");
        let p1_hash = b256!("2222222222222222222222222222222222222222222222222222222222222222");

        let parent_cp = b256!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let checkpoint0 = Checkpoint {
            blockNumber: 1,
            blockHash: b256!("0000000000000000000000000000000000000000000000000000000000000001"),
            stateRoot: b256!("0000000000000000000000000000000000000000000000000000000000000002"),
        };
        let checkpoint1 = Checkpoint {
            blockNumber: 2,
            blockHash: b256!("0000000000000000000000000000000000000000000000000000000000000003"),
            stateRoot: b256!("0000000000000000000000000000000000000000000000000000000000000004"),
        };
        let cp0 = checkpoint0.clone().blockHash;

        let mk = |proposal_id: u64,
                  proposal_hash: B256,
                  parent_proposal_hash: B256,
                  parent_block_hash: B256,
                  checkpoint: Checkpoint| {
            ProofCarryData {
                chain_id,
                verifier,
                transition_input: TransitionInputData {
                    proposal_id,
                    proposal_hash,
                    parent_proposal_hash,
                    parent_block_hash,
                    // Not relevant for these checks
                    actual_prover: address!("1111111111111111111111111111111111111111"),
                    transition: ShastaTransitionInput {
                        proposer: address!("2222222222222222222222222222222222222222"),
                        timestamp: 123,
                    },
                    checkpoint,
                },
            }
        };

        // Happy path: ids increment, proposal hash chains, checkpoint continuity holds.
        let carry_ok = vec![
            mk(
                1,
                p0_hash,
                b256!("0000000000000000000000000000000000000000000000000000000000000000"),
                parent_cp,
                checkpoint0.clone(),
            ),
            mk(2, p1_hash, p0_hash, cp0, checkpoint1.clone()),
        ];
        let proofs_ok = vec![
            RawProof {
                proof: vec![0u8],
                input: b256!("0000000000000000000000000000000000000000000000000000000000000000"),
            },
            RawProof {
                proof: vec![1u8],
                input: b256!("0000000000000000000000000000000000000000000000000000000000000000"),
            },
        ];
        assert!(validate_shasta_aggregate_proof_carry_data(
            &ShastaRawAggregationGuestInput {
                proofs: proofs_ok.clone(),
                proof_carry_data_vec: carry_ok,
            }
        ));

        // Mismatched lengths => invalid
        assert!(!validate_shasta_aggregate_proof_carry_data(
            &ShastaRawAggregationGuestInput {
                proofs: proofs_ok.clone(),
                proof_carry_data_vec: vec![],
            }
        ));

        // Non-sequential ids => invalid
        let carry_bad_ids = vec![
            mk(
                1,
                p0_hash,
                b256!("0000000000000000000000000000000000000000000000000000000000000000"),
                parent_cp,
                checkpoint0.clone(),
            ),
            mk(3, p1_hash, p0_hash, cp0, checkpoint1.clone()),
        ];
        assert!(!validate_shasta_aggregate_proof_carry_data(
            &ShastaRawAggregationGuestInput {
                proofs: proofs_ok.clone(),
                proof_carry_data_vec: carry_bad_ids,
            }
        ));

        // Broken proposal-hash chaining => invalid
        let carry_bad_hash_chain = vec![
            mk(
                1,
                p0_hash,
                b256!("0000000000000000000000000000000000000000000000000000000000000000"),
                parent_cp,
                checkpoint0,
            ),
            mk(
                2,
                p1_hash,
                b256!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
                cp0,
                checkpoint1,
            ),
        ];
        assert!(!validate_shasta_aggregate_proof_carry_data(
            &ShastaRawAggregationGuestInput {
                proofs: proofs_ok,
                proof_carry_data_vec: carry_bad_hash_chain,
            }
        ));
    }
}
