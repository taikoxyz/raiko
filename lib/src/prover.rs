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

pub trait Prover {
    type ProofParam: fmt::Debug + Clone;
    type ProofResponse: Serialize;

    async fn run(
        input: GuestInput,
        output: GuestOutput,
        param: Self::ProofParam,
    ) -> ProverResult<Self::ProofResponse>;

    fn instance_hash(pi: ProtocolInstance) -> B256;
}
