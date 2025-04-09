use reth_primitives::{ChainId, B256};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::input::{
    AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput, GuestInput,
    GuestOutput,
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
}

// impl display for proof to easy read log
impl std::fmt::Display for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Proof {{ proof: {}, input: {}, quote: {}, uuid: {}, kzg_proof: {} }}",
            self.proof.as_ref().unwrap_or(&"None".to_string()),
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

    async fn cancel(&self, proof_key: ProofKey, read: Box<&mut dyn IdStore>) -> ProverResult<()>;
}
