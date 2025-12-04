//! Shasta protocol types.
//!
//! These types mirror the Solidity structures from the Taiko Shasta contracts.

use alloy_primitives::{Address, B256};
use serde::{Deserialize, Serialize};

/// Proposal structure from IInbox.Proposal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: u64,
    pub timestamp: u64,
    pub end_of_submission_window_timestamp: u64,
    pub proposer: Address,
    pub core_state_hash: B256,
    pub derivation_hash: B256,
}

/// Derivation structure from IInbox.Derivation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Derivation {
    pub origin_block_number: u64,
    pub origin_block_hash: B256,
    pub basefee_sharing_pctg: u8,
    pub sources: Vec<DerivationSource>,
}

/// DerivationSource structure from IInbox.DerivationSource.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivationSource {
    pub blob_slice: BlobSlice,
    pub flags: u8,
}

/// BlobSlice structure from LibBlobs.BlobSlice.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobSlice {
    pub blob_hashes: Vec<B256>,
    pub offset: u32,
    pub timestamp: u64,
}

/// CoreState structure from IInbox.CoreState.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreState {
    pub next_proposal_id: u64,
    pub last_proposal_block_id: u64,
    pub last_finalized_proposal_id: u64,
    pub last_checkpoint_timestamp: u64,
    pub last_finalized_transition_hash: B256,
    pub bond_instructions_hash: B256,
}

/// Checkpoint structure from ICheckpointStore.Checkpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub block_number: u64,
    pub block_hash: B256,
    pub state_root: B256,
}

/// Transition structure from IInbox.Transition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub proposal_hash: B256,
    pub parent_transition_hash: B256,
    pub checkpoint: Checkpoint,
}

/// TransitionRecord structure from IInbox.TransitionRecord.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionRecord {
    pub span: u8,
    pub bond_instructions: Vec<BondInstruction>,
    pub transition_hash: B256,
    pub checkpoint_hash: B256,
}

/// TransitionMetadata structure from IInbox.TransitionMetadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionMetadata {
    pub designated_prover: Address,
    pub actual_prover: Address,
}

/// BondInstruction structure from LibBonds.BondInstruction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BondInstruction {
    pub proposal_id: u64,
    pub bond_type: u8,
    pub payer: Address,
    pub payee: Address,
}

/// Proposed event payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedEventPayload {
    pub proposal: Proposal,
    pub derivation: Derivation,
    pub core_state: CoreState,
    pub bond_instructions: Vec<BondInstruction>,
}

/// Proved event payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvedEventPayload {
    pub proposal_id: u64,
    pub transition: Transition,
    pub transition_record: TransitionRecord,
    pub metadata: TransitionMetadata,
}
