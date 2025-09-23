use reth_primitives::{Address, ChainId, B256};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput,
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
pub type ProofExtraData = (ChainId, Address);

#[derive(
    Clone, Debug, Serialize, ToSchema, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
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
    pub extra_data: Option<ProofExtraData>,
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
        let verifier_address = input
            .taiko
            .chain_spec
            .get_fork_verifier_address(input.inputs.first().unwrap().block.number, proof_type)
            .unwrap_or_default();
        proof.extra_data = Some((chain_id, verifier_address));
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
