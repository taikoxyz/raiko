use std::fmt;

use alloy_primitives::B256;
use serde::Serialize;
use thiserror::Error as ThisError;

use crate::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
};

#[derive(ThisError, Debug)]
pub enum ProverError {
    GuestError(String),
}

impl fmt::Display for ProverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProverError::GuestError(e) => e.fmt(f),
        }
    }
}

impl From<String> for ProverError {
    fn from(e: String) -> Self {
        ProverError::GuestError(e)
    }
}

pub type ProverResult<T, E = ProverError> = core::result::Result<T, E>;
pub type ProverConfig = serde_json::Value;
pub type Proof = serde_json::Value;

pub trait Prover {
    #[allow(async_fn_in_trait)]
    async fn run(
        input: GuestInput,
        output: GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof>;

    fn instance_hash(pi: ProtocolInstance) -> B256;
}

pub fn to_proof(proof: ProverResult<impl Serialize>) -> ProverResult<Proof> {
    proof.and_then(|res| {
        serde_json::to_value(res).map_err(|err| ProverError::GuestError(err.to_string()))
    })
}
