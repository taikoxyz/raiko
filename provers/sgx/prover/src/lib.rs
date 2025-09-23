#![cfg(feature = "enable")]

use std::{
    collections::HashMap,
    str::{self},
};

use raiko_lib::{
    consts::SpecId,
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput,
    },
    primitives::B256,
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverResult},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

pub mod local_prover;
use local_prover::LocalSgxProver;
mod remote_prover;
use remote_prover::RemoteSgxProver;
use tracing::debug;
// to register the instance id
mod sgx_register_utils;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SgxParam {
    pub instance_ids: HashMap<SpecId, u64>,
    pub setup: bool,
    pub bootstrap: bool,
    pub prove: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
    pub input: B256,
}

impl From<SgxResponse> for Proof {
    fn from(value: SgxResponse) -> Self {
        Self {
            proof: Some(value.proof),
            input: Some(value.input),
            quote: Some(value.quote),
            uuid: None,
            kzg_proof: None,
            extra_data: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
enum SgxProverType {
    /// Local SGX prover
    /// This is the default prover.
    #[default]
    Local,
    /// Remote SGX prover
    Remote,
}

impl std::str::FromStr for SgxProverType {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local" => Ok(SgxProverType::Local),
            "remote" => Ok(SgxProverType::Remote),
            _ => unimplemented!("unknown sgx mode"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SgxProver {
    /// Local SGX prover
    /// This is the default prover.
    Local(LocalSgxProver),
    /// Remote SGX prover
    Remote(RemoteSgxProver),
}

impl SgxProver {
    pub fn new(prove_type: ProofType) -> Self {
        let service_type = &std::env::var("SGX_MODE")
            .unwrap_or_else(|_| "local".to_string())
            .parse::<SgxProverType>()
            .unwrap_or_default();
        debug!("sgx mode: {:?}, prove_type: {}", service_type, prove_type);
        let prover = match service_type {
            SgxProverType::Local => SgxProver::Local(local_prover::LocalSgxProver::new(prove_type)),
            SgxProverType::Remote => {
                SgxProver::Remote(remote_prover::RemoteSgxProver::new(prove_type))
            }
        };
        prover
    }
}

impl Prover for SgxProver {
    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.run(input, output, config, store).await,
            SgxProver::Remote(prover) => prover.run(input, output, config, store).await,
        }
    }
    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.batch_run(input, output, config, store).await,
            SgxProver::Remote(prover) => prover.batch_run(input, output, config, store).await,
        }
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.aggregate(input, output, config, store).await,
            SgxProver::Remote(prover) => prover.aggregate(input, output, config, store).await,
        }
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.shasta_aggregate(input, output, config, store).await,
            SgxProver::Remote(prover) => prover.shasta_aggregate(input, output, config, store).await,
        }
    }

    async fn cancel(&self, proof_key: ProofKey, read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        match self {
            SgxProver::Local(prover) => prover.cancel(proof_key, read).await,
            SgxProver::Remote(prover) => prover.cancel(proof_key, read).await,
        }
    }

    fn proof_type(&self) -> ProofType {
        match self {
            SgxProver::Local(prover) => prover.proof_type(),
            SgxProver::Remote(prover) => prover.proof_type(),
        }
    }
}
