use reth_primitives::{ChainId, B256};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::input::{GuestInput, GuestOutput};

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
pub type ProofKey = (ChainId, B256);

#[derive(Debug, Serialize, ToSchema, Deserialize, Default)]
/// The response body of a proof request.
pub struct Proof {
    /// The ZK proof.
    pub proof: Option<String>,
    /// The TEE quote.
    pub quote: Option<String>,
    /// The kzg proof.
    pub kzg_proof: Option<String>,
}

pub trait IdWrite {
    fn store_id(&mut self, key: ProofKey, id: String) -> ProverResult<()>;

    fn remove_id(&mut self, key: ProofKey) -> ProverResult<()>;
}

pub trait IdStore: IdWrite {
    fn read_id(&self, key: ProofKey) -> ProverResult<String>;
}

#[allow(async_fn_in_trait)]
pub trait Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        store: &mut dyn IdWrite,
    ) -> ProverResult<Proof>;

    async fn cancel(proof_key: ProofKey, read: &mut dyn IdStore) -> ProverResult<()>;
}
