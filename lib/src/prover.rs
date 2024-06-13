use serde::Serialize;
use thiserror::Error as ThisError;

use crate::input::{GuestInput, GuestOutput};

#[derive(ThisError, Debug)]
pub enum ProverError {
    #[error("ProverError::GuestError `{0}`")]
    GuestError(String),
    #[error("ProverError::FileIo `{0}`")]
    FileIo(#[from] std::io::Error),
    #[error("ProverError::Param `{0}`")]
    Param(#[from] serde_json::Error),
}

impl From<String> for ProverError {
    fn from(e: String) -> Self {
        ProverError::GuestError(e)
    }
}

pub type ProverResult<T, E = ProverError> = core::result::Result<T, E>;
pub type ProverConfig = serde_json::Value;
pub type Proof = serde_json::Value;

#[allow(async_fn_in_trait)]
pub trait Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof>;
}

pub fn to_proof(proof: ProverResult<impl Serialize>) -> ProverResult<Proof> {
    proof.and_then(|res| {
        serde_json::to_value(res).map_err(|err| ProverError::GuestError(err.to_string()))
    })
}
