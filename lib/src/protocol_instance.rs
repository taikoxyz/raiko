use std::collections::HashSet;

use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::SolValue;
use anyhow::{ensure, Ok, Result};
use reth_evm_ethereum::taiko::decode_anchor_shasta;
use reth_primitives::{Block, Header};

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{
    consts::SupportedChainSpecs,
    input::{
        shasta::{Checkpoint, Commitment, Proposal as ShastaProposal},
        BlobProofType, BlockProposedFork, GuestBatchInput, GuestInput,
        ShastaRawAggregationGuestInput,
    },
    libhash::{
        hash_commitment, hash_proposal, hash_public_input, hash_shasta_subproof_input,
        hash_two_values,
    },
    primitives::{
        eip4844::{self, commitment_to_version_hash},
        keccak::keccak,
        mpt::{MptNode, StateAccount},
    },
    proof_type::ProofType,
    prover::{ProofCarryData, ShastaTransitionInput, TransitionInputData},
    CycleTracker,
};
use tracing::{debug, error, info};
#[derive(Debug, Clone)]
pub enum BlockMetaDataFork {
    None,
    Shasta(ShastaProposal),
}

impl BlockMetaDataFork {
    fn from_batch_inputs(batch_input: &GuestBatchInput, _final_blocks: Vec<Block>) -> Self {
        match &batch_input.taiko.batch_proposed {
            BlockProposedFork::Shasta(event_data) => {
                BlockMetaDataFork::Shasta(event_data.proposal.clone())
            }
            _ => unimplemented!("batch blocks signature is only supported for shasta"),
        }
    }

    fn match_block_proposal(&self, other: &BlockProposedFork) -> bool {
        matches!(
            (self, other),
            (Self::Shasta(_), BlockProposedFork::Shasta(_))
                | (Self::None, BlockProposedFork::Nothing)
        )
    }
}

#[derive(Debug, Clone)]
pub enum TransitionFork {
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
            let (x, y) = eip4844::proof_of_equivalence(blob_data, expected_versioned_hash)?;
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
                *expected_versioned_hash == commitment_to_version_hash(commitment),
                "blob versioned hash mismatch"
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
    // Expected on-chain blob hashes for each Shasta derivation source.
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

/// Validates input chain spec against known specs when chain_id is recognized.
/// For unknown chain ids, skips check to allow tests with custom data.
fn validate_known_chain_spec(chain_spec: &crate::consts::ChainSpec) -> Result<()> {
    let Some(verified) =
        SupportedChainSpecs::default().get_chain_spec_with_chain_id(chain_spec.chain_id)
    else {
        return Ok(());
    };
    ensure!(
        chain_spec.name == verified.name,
        "unexpected name: {:?}, expected: {:?}",
        chain_spec.name,
        verified.name
    );
    ensure!(
        chain_spec.max_spec_id == verified.max_spec_id,
        "unexpected max_spec_id"
    );
    ensure!(
        chain_spec.hard_forks == verified.hard_forks,
        "unexpected hard_forks: {:?}, expected: {:?}",
        chain_spec.hard_forks,
        verified.hard_forks
    );
    ensure!(
        chain_spec.eip_1559_constants == verified.eip_1559_constants,
        "unexpected eip_1559_constants"
    );
    ensure!(
        chain_spec.l1_contract == verified.l1_contract,
        "unexpected l1_contract"
    );
    ensure!(
        chain_spec.l2_contract == verified.l2_contract,
        "unexpected l2_contract"
    );
    ensure!(
        chain_spec.is_taiko == verified.is_taiko,
        "unexpected is_taiko"
    );
    Ok(())
}

const SHASTA_ANCHOR_PREDEPLOY_SUFFIX: [u8; 3] = [0x01, 0x00, 0x01];
// L2 SignalService proxy uses the same chain-id predeploy prefix as Anchor with suffix 0005.
const SHASTA_SIGNAL_SERVICE_PREDEPLOY_SUFFIX: [u8; 3] = [0x00, 0x00, 0x05];
// SignalService_Layout.sol: mapping(uint48 => CheckpointRecord) _checkpoints at slot 254.
const SHASTA_SIGNAL_SERVICE_CHECKPOINTS_SLOT: u64 = 254;

pub fn shasta_signal_service_address_from_l2_contract(
    l2_contract: Option<Address>,
) -> Option<Address> {
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(l2_contract?.as_slice());
    if bytes[17..20] != SHASTA_ANCHOR_PREDEPLOY_SUFFIX {
        return None;
    }
    bytes[17..20].copy_from_slice(&SHASTA_SIGNAL_SERVICE_PREDEPLOY_SUFFIX);
    Some(Address::from_slice(&bytes))
}

pub fn shasta_checkpoint_storage_slots(block_number: u64) -> (U256, U256) {
    let mut encoded = Vec::with_capacity(64);
    encoded.extend_from_slice(&U256::from(block_number).to_be_bytes::<32>());
    encoded
        .extend_from_slice(&U256::from(SHASTA_SIGNAL_SERVICE_CHECKPOINTS_SLOT).to_be_bytes::<32>());
    let block_hash_slot = U256::from_be_bytes::<32>(keccak(&encoded));
    let state_root_slot = block_hash_slot + U256::from(1);
    (block_hash_slot, state_root_slot)
}

fn read_storage_b256(storage_trie: &MptNode, slot: U256) -> Option<B256> {
    let value = storage_trie
        .get_rlp::<U256>(&keccak(slot.to_be_bytes::<32>()))
        .ok()
        .flatten()
        .unwrap_or_default();
    Some(B256::from_slice(&value.to_be_bytes::<32>()))
}

fn read_parent_shasta_checkpoint(input: &GuestInput, block_number: u64) -> Option<Checkpoint> {
    let signal_service =
        shasta_signal_service_address_from_l2_contract(input.chain_spec.l2_contract)?;
    let (block_hash_slot, state_root_slot) = shasta_checkpoint_storage_slots(block_number);
    let (storage_trie, _) = input.parent_storage.get(&signal_service)?;
    if input.parent_state_trie.hash() != input.parent_header.state_root {
        error!(
            "cannot read parent shasta checkpoint: parent state trie root mismatch, expected: {:?}, got: {:?}",
            input.parent_header.state_root,
            input.parent_state_trie.hash()
        );
        return None;
    }
    let state_account = input
        .parent_state_trie
        .get_rlp::<StateAccount>(&keccak(signal_service))
        .ok()
        .flatten()?;
    if storage_trie.hash() != state_account.storage_root {
        error!(
            "cannot read parent shasta checkpoint: signal service storage root mismatch, expected: {:?}, got: {:?}",
            state_account.storage_root,
            storage_trie.hash()
        );
        return None;
    }
    let block_hash = read_storage_b256(storage_trie, block_hash_slot)?;
    let state_root = read_storage_b256(storage_trie, state_root_slot)?;
    if block_hash == B256::ZERO || state_root == B256::ZERO {
        return None;
    }
    Some(Checkpoint {
        blockNumber: block_number,
        blockHash: block_hash,
        stateRoot: state_root,
    })
}

fn bypass_shasta_anchor_linkage(batch_input: &GuestBatchInput) -> bool {
    if !batch_input.taiko.l1_ancestor_headers.is_empty() || batch_input.inputs.is_empty() {
        return false;
    }
    let Some(last_anchor_block_number) = batch_input.taiko.prover_data.last_anchor_block_number
    else {
        return false;
    };
    let Some(parent_checkpoint) =
        read_parent_shasta_checkpoint(&batch_input.inputs[0], last_anchor_block_number)
    else {
        error!("cannot bypass shasta anchor linkage: missing parent checkpoint");
        return false;
    };

    for input in &batch_input.inputs {
        let Some(anchor_tx) = input.taiko.anchor_tx.as_ref() else {
            error!("cannot bypass shasta anchor linkage: missing anchor tx");
            return false;
        };
        let std::result::Result::Ok(anchor_data) = decode_anchor_shasta(anchor_tx.input()) else {
            error!("cannot bypass shasta anchor linkage: failed to decode anchor tx");
            return false;
        };
        let anchor_checkpoint = Checkpoint {
            blockNumber: anchor_data._checkpoint.blockNumber,
            blockHash: anchor_data._checkpoint.blockHash,
            stateRoot: anchor_data._checkpoint.stateRoot,
        };
        if anchor_checkpoint != parent_checkpoint {
            error!(
                "cannot bypass shasta anchor linkage: anchor checkpoint mismatch, expected: {:?}, got: {:?}",
                parent_checkpoint, anchor_checkpoint
            );
            return false;
        }
    }
    true
}

#[cfg(test)]
fn is_stalled_anchor_bypass_allowed(
    l1_ancestor_headers_empty: bool,
    last_anchor_block_number: Option<u64>,
    anchor_block_numbers: &[u64],
) -> bool {
    if !l1_ancestor_headers_empty || anchor_block_numbers.is_empty() {
        return false;
    }
    let Some(last_anchor_block_number) = last_anchor_block_number else {
        return false;
    };
    anchor_block_numbers
        .iter()
        .all(|anchor_block_number| *anchor_block_number == last_anchor_block_number)
}

impl ProtocolInstance {
    pub fn new(_input: &GuestInput, _header: &Header, _proof_type: ProofType) -> Result<Self> {
        Err(anyhow::Error::msg(
            "single block protocol instances are not supported in shasta-only mode",
        ))
    }

    pub fn new_batch(
        batch_input: &GuestBatchInput,
        blocks: Vec<Block>,
        proof_type: ProofType,
    ) -> Result<Self> {
        // verify blob usage, either by commitment or proof equality.
        verify_batch_mode_blob_usage(batch_input, proof_type)?;

        for input in &batch_input.inputs {
            validate_known_chain_spec(&input.chain_spec)?;
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

        // Sanity check: block metadata must match proposal
        ensure!(
            pi.block_metadata
                .match_block_proposal(&batch_input.taiko.batch_proposed),
            "batch block hash mismatch"
        );

        Ok(pi)
    }

    pub fn sgx_instance(mut self, instance: Address) -> Self {
        self.sgx_instance = instance;
        self
    }

    pub fn meta_hash(&self) -> B256 {
        match self.block_metadata {
            BlockMetaDataFork::None => keccak(vec![]).into(),
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

        let TransitionFork::Shasta(shasta_trans_input) = &self.transition;
        info!(
            "transition to be signed into public: {:?}.",
            shasta_trans_input
        );
        hash_shasta_subproof_input(&ProofCarryData {
            chain_id: self.chain_id,
            verifier: self.verifier_address,
            transition_input: shasta_trans_input.clone(),
        })
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
        let msg = format!(
            "shasta PCD validation failed: proofs.len()={} != proof_carry_data_vec.len()={}",
            aggregation_input.proofs.len(),
            aggregation_input.proof_carry_data_vec.len()
        );
        error!("{msg}");
        return false;
    }
    validate_shasta_proof_carry_data_vec(&aggregation_input.proof_carry_data_vec)
}

pub fn validate_shasta_proof_carry_data_vec(proof_carry_data_vec: &[ProofCarryData]) -> bool {
    if proof_carry_data_vec.is_empty() {
        let msg = "shasta PCD validation failed: empty proof_carry_data_vec";
        error!("{msg}");
        return false;
    }

    let proposal_ids: Vec<u64> = proof_carry_data_vec
        .iter()
        .map(|p| p.transition_input.proposal_id)
        .collect();
    debug!(
        "validating shasta PCD: len={}, proposal_ids={:?}, actual_prover={:?}",
        proof_carry_data_vec.len(),
        proposal_ids,
        proof_carry_data_vec[0].transition_input.actual_prover
    );

    let expected_actual_prover = proof_carry_data_vec[0].transition_input.actual_prover;
    for (i, item) in proof_carry_data_vec.iter().enumerate() {
        // Commitment uses a single `actualProver` field; make the range unambiguous.
        if item.transition_input.actual_prover != expected_actual_prover {
            let msg = format!(
                "shasta PCD validation failed at index {}: mismatched actual_prover, got {:?}, expected {:?}",
                i,
                item.transition_input.actual_prover,
                expected_actual_prover
            );
            error!("{msg}");
            return false;
        }
    }

    for (i, w) in proof_carry_data_vec.windows(2).enumerate() {
        let prev = &w[0];
        let next = &w[1];
        // Ensure proposal ids are sequential
        if prev.transition_input.proposal_id + 1 != next.transition_input.proposal_id {
            let msg = format!(
                "shasta PCD validation failed at pair [{},{}]: proposal_id not sequential, prev={}, next={} (expected {})",
                i, i + 1,
                prev.transition_input.proposal_id,
                next.transition_input.proposal_id,
                prev.transition_input.proposal_id + 1
            );
            error!("{msg}");
            return false;
        }

        // Ensure proposal hashes chain correctly
        if prev.transition_input.proposal_hash != next.transition_input.parent_proposal_hash {
            let msg = format!(
                "shasta PCD validation failed at pair [{},{}]: proposal_hash chain broken, prev.proposal_hash={:?} != next.parent_proposal_hash={:?}",
                i, i + 1,
                prev.transition_input.proposal_hash,
                next.transition_input.parent_proposal_hash
            );
            error!("{msg}");
            return false;
        }

        if prev.chain_id != next.chain_id {
            let msg =
                format!(
                "shasta PCD validation failed at pair [{},{}]: chain_id mismatch, prev={}, next={}",
                i, i + 1, prev.chain_id, next.chain_id
            );
            error!("{msg}");
            return false;
        }

        if prev.verifier != next.verifier {
            let msg = format!(
                "shasta PCD validation failed at pair [{},{}]: verifier mismatch, prev={:?}, next={:?}",
                i, i + 1, prev.verifier, next.verifier
            );
            error!("{msg}");
            return false;
        }

        // Continuity: prev checkpoint must match next parent checkpoint hash.
        if prev.transition_input.checkpoint.blockHash != next.transition_input.parent_block_hash {
            let msg = format!(
                "shasta PCD validation failed at pair [{},{}]: checkpoint block hash chain broken, prev.checkpoint.blockHash={:?} != next.parent_block_hash={:?}",
                i, i + 1,
                prev.transition_input.checkpoint.blockHash,
                next.transition_input.parent_block_hash
            );
            error!("{msg}");
            return false;
        }
    }

    true
}

pub fn build_shasta_commitment_from_proof_carry_data_vec(
    proof_carry_data_vec: &[ProofCarryData],
) -> Option<Commitment> {
    if !validate_shasta_proof_carry_data_vec(proof_carry_data_vec) {
        error!(
            "build_shasta_commitment failed: validate_shasta_proof_carry_data_vec returned false (see above for specific reason)"
        );
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
    let prove_input_hash = hash_commitment(prove_input);
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
    use std::collections::HashMap;

    use alloy_primitives::{address, b256, Bytes, U256};
    use alloy_sol_types::SolCall;
    use reth_evm_ethereum::taiko::anchorV4Call;
    use reth_primitives::{Header, Signature, TransactionSigned, TxKind, TxLegacy};

    use super::*;
    use crate::{
        input::{shasta::Checkpoint, TaikoGuestBatchInput, TaikoProverData},
        primitives::mpt::MptNode,
    };

    #[test]
    fn stalled_anchor_bypass_requires_last_anchor_match() {
        assert!(is_stalled_anchor_bypass_allowed(
            true,
            Some(100),
            &[100, 100]
        ));
        assert!(!is_stalled_anchor_bypass_allowed(
            true,
            Some(100),
            &[101, 101]
        ));
        assert!(!is_stalled_anchor_bypass_allowed(
            true,
            Some(100),
            &[100, 101]
        ));
        assert!(!is_stalled_anchor_bypass_allowed(false, Some(100), &[100]));
        assert!(!is_stalled_anchor_bypass_allowed(true, None, &[100]));
        assert!(!is_stalled_anchor_bypass_allowed(true, Some(100), &[]));
    }

    fn test_checkpoint_storage_slots(block_number: u64) -> (U256, U256) {
        shasta_checkpoint_storage_slots(block_number)
    }

    fn test_anchor_tx(checkpoint: Checkpoint) -> TransactionSigned {
        let input = anchorV4Call {
            _checkpoint: reth_evm_ethereum::taiko::Checkpoint {
                blockNumber: checkpoint.blockNumber,
                blockHash: checkpoint.blockHash,
                stateRoot: checkpoint.stateRoot,
            },
        }
        .abi_encode();
        let tx = TxLegacy {
            to: TxKind::Call(address!("1670010000000000000000000000000000010001")),
            input: Bytes::from(input),
            ..Default::default()
        };
        TransactionSigned::from_transaction_and_signature(tx.into(), Signature::default())
    }

    fn test_input_with_parent_checkpoint(
        anchor_tx_checkpoint: Checkpoint,
        parent_checkpoint: Checkpoint,
    ) -> GuestInput {
        let signal_service = address!("1670010000000000000000000000000000000005");
        let (block_hash_slot, state_root_slot) =
            test_checkpoint_storage_slots(parent_checkpoint.blockNumber);
        let mut storage_trie = MptNode::default();
        storage_trie
            .insert_rlp(
                &keccak(block_hash_slot.to_be_bytes::<32>()),
                U256::from_be_bytes::<32>(*parent_checkpoint.blockHash),
            )
            .unwrap();
        storage_trie
            .insert_rlp(
                &keccak(state_root_slot.to_be_bytes::<32>()),
                U256::from_be_bytes::<32>(*parent_checkpoint.stateRoot),
            )
            .unwrap();
        let mut parent_state_trie = MptNode::default();
        parent_state_trie
            .insert_rlp(
                &keccak(signal_service),
                StateAccount {
                    storage_root: storage_trie.hash(),
                    ..Default::default()
                },
            )
            .unwrap();

        let mut parent_storage = HashMap::new();
        parent_storage.insert(
            signal_service,
            (storage_trie, vec![block_hash_slot, state_root_slot]),
        );

        GuestInput {
            chain_spec: crate::consts::ChainSpec {
                l2_contract: Some(address!("1670010000000000000000000000000000010001")),
                ..Default::default()
            },
            parent_header: Header {
                state_root: parent_state_trie.hash(),
                ..Default::default()
            },
            parent_state_trie,
            parent_storage,
            taiko: crate::input::TaikoGuestInput {
                anchor_tx: Some(test_anchor_tx(anchor_tx_checkpoint)),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn stalled_anchor_bypass_rejects_checkpoint_value_mismatch() {
        let parent_checkpoint = Checkpoint {
            blockNumber: 100,
            blockHash: b256!("1111111111111111111111111111111111111111111111111111111111111111"),
            stateRoot: b256!("2222222222222222222222222222222222222222222222222222222222222222"),
        };
        let forged_checkpoint = Checkpoint {
            blockNumber: parent_checkpoint.blockNumber,
            blockHash: b256!("3333333333333333333333333333333333333333333333333333333333333333"),
            stateRoot: parent_checkpoint.stateRoot,
        };
        let batch_input = GuestBatchInput {
            inputs: vec![test_input_with_parent_checkpoint(
                forged_checkpoint,
                parent_checkpoint,
            )],
            taiko: TaikoGuestBatchInput {
                prover_data: TaikoProverData {
                    last_anchor_block_number: Some(100),
                    ..Default::default()
                },
                ..Default::default()
            },
        };

        assert!(!bypass_shasta_anchor_linkage(&batch_input));
    }

    #[test]
    fn stalled_anchor_bypass_accepts_parent_checkpoint_match() {
        let parent_checkpoint = Checkpoint {
            blockNumber: 100,
            blockHash: b256!("1111111111111111111111111111111111111111111111111111111111111111"),
            stateRoot: b256!("2222222222222222222222222222222222222222222222222222222222222222"),
        };
        let batch_input = GuestBatchInput {
            inputs: vec![test_input_with_parent_checkpoint(
                parent_checkpoint.clone(),
                parent_checkpoint,
            )],
            taiko: TaikoGuestBatchInput {
                prover_data: TaikoProverData {
                    last_anchor_block_number: Some(100),
                    ..Default::default()
                },
                ..Default::default()
            },
        };

        assert!(bypass_shasta_anchor_linkage(&batch_input));
    }

    #[test]
    fn known_chain_spec_rejects_name_mismatch() {
        let mut chain_spec = crate::consts::SupportedChainSpecs::default()
            .get_chain_spec("taiko_mainnet")
            .unwrap();
        chain_spec.name = "taiko_dev".to_string();
        let batch_input = GuestBatchInput {
            inputs: vec![GuestInput {
                chain_spec,
                ..Default::default()
            }],
            ..Default::default()
        };

        let err = ProtocolInstance::new_batch(&batch_input, Vec::new(), ProofType::Sgx)
            .expect_err("chain spec name mismatch should be rejected");
        assert!(err.to_string().contains("unexpected name"));
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
