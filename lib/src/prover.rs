use reth_primitives::{Address, ChainId, B256};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    input::{
        shasta::Checkpoint, AggregationGuestInput, AggregationGuestOutput, GuestBatchInput,
        GuestBatchOutput, GuestInput, GuestOutput, ShastaAggregationGuestInput,
    },
    proof_type::ProofType,
};

#[derive(thiserror::Error, Debug)]
pub enum ProverError {
    #[error("ProverError::GuestError `{0}`")]
    GuestError(String),
    #[error("ProverError::FileIo `{0}`")]
    FileIo(#[from] std::io::Error),
    #[error("ProverError::Param `{0}`")]
    Param(#[from] serde_json::Error),
    #[error("Store error `{0}`")]
    StoreError(String),
}

impl From<String> for ProverError {
    fn from(e: String) -> Self {
        ProverError::GuestError(e)
    }
}

pub type ProverResult<T, E = ProverError> = core::result::Result<T, E>;
pub type ProverConfig = serde_json::Value;
pub type ProofKey = (ChainId, u64, B256, u8);

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
// In Shasta, each sub proposal signs this structure to prove the proposal's transition.
// We keep ABI-compatible field names.
pub struct ShastaTransitionInput {
    pub proposer: Address,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TransitionInputData {
    pub proposal_id: u64,
    pub proposal_hash: B256,
    pub parent_proposal_hash: B256,
    pub parent_block_hash: B256,
    pub actual_prover: Address,
    pub transition: ShastaTransitionInput,
    pub checkpoint: Checkpoint,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProofCarryData {
    pub chain_id: ChainId,
    pub verifier: Address,
    pub transition_input: TransitionInputData,
}

#[derive(Clone, Debug, Serialize, ToSchema, Deserialize, Default, PartialEq, Eq)]
/// The response body of a proof request.
pub struct Proof {
    /// The proof either TEE or ZK.
    pub proof: Option<String>,
    /// The public input
    pub input: Option<B256>,
    /// The TEE quote.
    pub quote: Option<String>,
    /// The assumption UUID.
    pub uuid: Option<String>,
    /// The kzg proof.
    pub kzg_proof: Option<String>,
    /// the extra data of Proof
    pub extra_data: Option<ProofCarryData>,
}

// impl display for proof to easy read log
impl std::fmt::Display for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Proof {{ proof: {}, input: {}, quote: {}, uuid: {}, kzg_proof: {} }}",
            self.proof
                .as_ref()
                .map(|p| {
                    if p.len() <= 120 {
                        format!("{}", p)
                    } else {
                        format!("{:?}...", p.chars().take(120).collect::<String>())
                    }
                })
                .unwrap_or("None".to_string()),
            self.input
                .as_ref()
                .map(|v| format!("{:?}", v))
                .unwrap_or("None".to_string()),
            self.quote
                .as_ref()
                .map(|v| format!("quote size:{}", v.len()))
                .unwrap_or("None".to_string()),
            self.uuid.as_ref().unwrap_or(&"None".to_string()),
            self.kzg_proof.as_ref().unwrap_or(&"None".to_string())
        ))
    }
}

#[async_trait::async_trait]
pub trait IdWrite: Send {
    async fn store_id(&mut self, key: ProofKey, id: String) -> ProverResult<()>;

    async fn remove_id(&mut self, key: ProofKey) -> ProverResult<()>;
}

#[async_trait::async_trait]
pub trait IdStore: IdWrite {
    async fn read_id(&mut self, key: ProofKey) -> ProverResult<String>;
}

#[allow(async_fn_in_trait)]
pub trait Prover {
    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof>;

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof>;

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof>;

    /// Run the prover for Shasta proposals (delegates to batch_run for now)
    async fn proposal_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Default implementation delegates to batch_run
        let mut proof = self.batch_run(input.clone(), output, config, store).await?;
        let proof_type = self.proof_type();
        let chain_id = input.taiko.chain_spec.chain_id;
        let first_block = &input.inputs.first().unwrap().block;
        let proposal_block_number = first_block.number;
        let first_block_timestamp = first_block.header.timestamp;
        let verifier_address = input
            .taiko
            .chain_spec
            .get_fork_verifier_address(proposal_block_number, first_block_timestamp, proof_type)
            .unwrap_or_default();
        let last_checkpoint = Checkpoint {
            blockNumber: input.inputs.last().unwrap().block.number,
            blockHash: input.inputs.last().unwrap().block.hash_slow(),
            stateRoot: input.inputs.last().unwrap().block.state_root,
        };
        proof.extra_data = Some(ProofCarryData {
            chain_id,
            verifier: verifier_address,
            transition_input: TransitionInputData {
                proposal_id: input.taiko.batch_id,
                proposal_hash: input.taiko.batch_proposed.proposal_hash(),
                parent_proposal_hash: input.taiko.batch_proposed.parent_proposal_hash(),
                parent_block_hash: input.inputs.first().unwrap().parent_header.hash_slow(),
                actual_prover: input.taiko.prover_data.actual_prover,
                transition: ShastaTransitionInput {
                    proposer: input.taiko.batch_proposed.proposer(),
                    timestamp: input.taiko.batch_proposed.proposal_timestamp(),
                },
                checkpoint: last_checkpoint,
            },
        });
        Ok(proof)
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof>;

    async fn cancel(&self, proof_key: ProofKey, read: Box<&mut dyn IdStore>) -> ProverResult<()>;

    fn proof_type(&self) -> ProofType;
}
