use crate::{merge, prover::NativeProver};
use alloy_primitives::{Address, B256};
use clap::Args;
use raiko_lib::{
    input::{
        shasta::Checkpoint, AggregationGuestInput, AggregationGuestOutput, BlobProofType,
        GuestBatchInput, GuestBatchOutput, GuestInput, GuestOutput, ShastaAggregationGuestInput,
    },
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverError},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{serde_as, DisplayFromStr};
use std::{collections::HashMap, fmt::Display, path::Path};
use utoipa::ToSchema;

#[derive(Debug, thiserror::Error, ToSchema)]
pub enum RaikoError {
    /// For invalid proof type generation request.
    #[error("Unknown proof type: {0}")]
    InvalidProofType(String),

    /// For invalid proof type generation request.
    #[error("Unknown proof type: {0}")]
    InvalidBlobOption(String),

    /// For invalid proof request configuration.
    #[error("Invalid proof request: {0}")]
    InvalidRequestConfig(String),

    /// For requesting a proof of a type that is not supported.
    #[error("Feature not supported: {0}")]
    #[schema(value_type = Value)]
    FeatureNotSupportedError(ProofType),

    /// For invalid type conversion.
    #[error("Invalid conversion: {0}")]
    Conversion(String),

    /// For RPC errors.
    #[error("There was an error with the RPC provider: {0}")]
    RPC(String),

    /// For preflight errors.
    #[error("There was an error running the preflight: {0}")]
    Preflight(String),

    /// For errors produced by the guest provers.
    #[error("There was an error with a guest prover: {0}")]
    #[schema(value_type = Value)]
    Guest(#[from] ProverError),

    /// For db errors.
    #[error("There was an error with the db: {0}")]
    #[schema(value_type = Value)]
    Db(raiko_lib::mem_db::DbError),

    /// For I/O errors.
    #[error("There was a I/O error: {0}")]
    #[schema(value_type = Value)]
    Io(#[from] std::io::Error),

    /// For Serde errors.
    #[error("There was a deserialization error: {0}")]
    #[schema(value_type = Value)]
    Serde(#[from] serde_json::Error),

    /// A catch-all error for any other error type.
    #[error("There was an unexpected error: {0}")]
    #[schema(value_type = Value)]
    Anyhow(#[from] anyhow::Error),
}

impl From<raiko_lib::mem_db::DbError> for RaikoError {
    fn from(e: raiko_lib::mem_db::DbError) -> Self {
        RaikoError::Db(e)
    }
}

pub type RaikoResult<T> = Result<T, RaikoError>;

/// Run the prover driver depending on the proof type.
pub async fn run_prover(
    proof_type: ProofType,
    input: GuestInput,
    output: &GuestOutput,
    config: &Value,
    store: Option<&mut dyn IdWrite>,
) -> RaikoResult<Proof> {
    match proof_type {
        ProofType::Native => NativeProver
            .run(input.clone(), output, config, store)
            .await
            .map_err(<ProverError as Into<RaikoError>>::into),
        ProofType::Sp1 => {
            #[cfg(feature = "sp1")]
            return sp1_driver::Sp1Prover
                .run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sp1"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Risc0 => {
            #[cfg(feature = "risc0")]
            return risc0_driver::Risc0Prover
                .run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "risc0"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Sgx | ProofType::SgxGeth => {
            #[cfg(feature = "sgx")]
            return sgx_prover::SgxProver::new(proof_type)
                .run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sgx"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
    }
}

/// Run the prover driver depending on the proof type.
pub async fn run_batch_prover(
    proof_type: ProofType,
    input: GuestBatchInput,
    output: &GuestBatchOutput,
    config: &Value,
    store: Option<&mut dyn IdWrite>,
) -> RaikoResult<Proof> {
    match proof_type {
        ProofType::Native => NativeProver
            .batch_run(input.clone(), output, config, store)
            .await
            .map_err(<ProverError as Into<RaikoError>>::into),
        ProofType::Sp1 => {
            #[cfg(feature = "sp1")]
            return sp1_driver::Sp1Prover
                .batch_run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sp1"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Risc0 => {
            #[cfg(feature = "risc0")]
            return risc0_driver::Risc0Prover
                .batch_run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "risc0"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Sgx | ProofType::SgxGeth => {
            #[cfg(feature = "sgx")]
            return sgx_prover::SgxProver::new(proof_type)
                .batch_run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sgx"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
    }
}

/// Run the prover driver for Shasta proposals depending on the proof type.
pub async fn run_shasta_proposal_prover(
    proof_type: ProofType,
    input: GuestBatchInput,
    output: &GuestBatchOutput,
    config: &Value,
    store: Option<&mut dyn IdWrite>,
) -> RaikoResult<Proof> {
    match proof_type {
        ProofType::Native => NativeProver
            .proposal_run(input.clone(), output, config, store)
            .await
            .map_err(<ProverError as Into<RaikoError>>::into),
        ProofType::Sp1 => {
            #[cfg(feature = "sp1")]
            return sp1_driver::Sp1Prover
                .proposal_run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sp1"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Risc0 => {
            #[cfg(feature = "risc0")]
            return risc0_driver::Risc0Prover
                .proposal_run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "risc0"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Sgx | ProofType::SgxGeth => {
            #[cfg(feature = "sgx")]
            return sgx_prover::SgxProver::new(proof_type)
                .proposal_run(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sgx"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
    }
}

/// Run the prover driver depending on the proof type.
pub async fn aggregate_proofs(
    proof_type: ProofType,
    input: AggregationGuestInput,
    output: &AggregationGuestOutput,
    config: &Value,
    store: Option<&mut dyn IdWrite>,
) -> RaikoResult<Proof> {
    let proof = match proof_type {
        ProofType::Native => NativeProver
            .aggregate(input.clone(), output, config, store)
            .await
            .map_err(<ProverError as Into<RaikoError>>::into),
        ProofType::Sp1 => {
            #[cfg(feature = "sp1")]
            return sp1_driver::Sp1Prover
                .aggregate(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sp1"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Risc0 => {
            #[cfg(feature = "risc0")]
            return risc0_driver::Risc0Prover
                .aggregate(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "risc0"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Sgx | ProofType::SgxGeth => {
            #[cfg(feature = "sgx")]
            return sgx_prover::SgxProver::new(proof_type)
                .aggregate(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sgx"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
    }?;

    Ok(proof)
}

pub async fn aggregate_shasta_proposals(
    proof_type: ProofType,
    input: ShastaAggregationGuestInput,
    output: &AggregationGuestOutput,
    config: &Value,
    store: Option<&mut dyn IdWrite>,
) -> RaikoResult<Proof> {
    let proof = match proof_type {
        ProofType::Native => NativeProver
            .shasta_aggregate(input.clone(), output, config, store)
            .await
            .map_err(<ProverError as Into<RaikoError>>::into),
        ProofType::Sp1 => {
            #[cfg(feature = "sp1")]
            return sp1_driver::Sp1Prover
                .shasta_aggregate(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sp1"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Risc0 => {
            #[cfg(feature = "risc0")]
            return risc0_driver::Risc0Prover
                .shasta_aggregate(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "risc0"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Sgx | ProofType::SgxGeth => {
            #[cfg(feature = "sgx")]
            return sgx_prover::SgxProver::new(proof_type)
                .shasta_aggregate(input.clone(), output, config, store)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sgx"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
    }?;

    Ok(proof)
}

pub async fn cancel_proof(
    proof_type: ProofType,
    proof_key: ProofKey,
    read: Box<&mut dyn IdStore>,
) -> RaikoResult<()> {
    match proof_type {
        ProofType::Native => NativeProver
            .cancel(proof_key, read)
            .await
            .map_err(<ProverError as Into<RaikoError>>::into),
        ProofType::Sp1 => {
            #[cfg(feature = "sp1")]
            return sp1_driver::Sp1Prover
                .cancel(proof_key, read)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sp1"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Risc0 => {
            #[cfg(feature = "risc0")]
            return risc0_driver::Risc0Prover
                .cancel(proof_key, read)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "risc0"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
        ProofType::Sgx | ProofType::SgxGeth => {
            #[cfg(feature = "sgx")]
            return sgx_prover::SgxProver::new(proof_type)
                .cancel(proof_key, read)
                .await
                .map_err(|e| e.into());
            #[cfg(not(feature = "sgx"))]
            Err(RaikoError::FeatureNotSupportedError(proof_type))
        }
    }?;
    Ok(())
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
/// A request for a proof.
pub struct ProofRequest {
    /// The block number for the block to generate a proof for.
    pub block_number: u64,
    /// The block number for the block to generate a proof for.
    /// in shasta, this is the proposal id
    pub batch_id: u64,
    /// The l1 block number of the l2 block be proposed.
    pub l1_inclusion_block_number: u64,
    /// To support batch proof generation.
    pub l2_block_numbers: Vec<u64>,
    /// The network to generate the proof for.
    pub network: String,
    /// The L1 network to generate the proof for.
    pub l1_network: String,
    /// Graffiti.
    pub graffiti: B256,
    /// The protocol instance data.
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,
    /// The proof type.
    pub proof_type: ProofType,
    /// Blob proof type.
    pub blob_proof_type: BlobProofType,
    #[serde(flatten)]
    /// Additional prover params.
    pub prover_args: HashMap<String, Value>,
    /// checkpoint, if not provided, it will be set to the default value
    /// in shasta, this is the checkpoint of the l2 block
    pub checkpoint: Option<ShastaProposalCheckpoint>,
    /// last anchor number
    pub last_anchor_block_number: Option<u64>,
    /// Cached block proposed event data to avoid duplicate RPC calls
    #[serde(skip)]
    pub cached_event_data: Option<raiko_lib::input::BlockProposedFork>,
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BatchMetadata {
    pub batch_id: u64,
    pub l1_inclusion_block_number: u64,
}

impl std::str::FromStr for BatchMetadata {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid BatchMetadata format. Expected 'batch_id:l1_inclusion_block_number'"
            ));
        }

        let batch_id = parts[0]
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("Invalid batch_id"))?;
        let l1_inclusion_block_number = parts[1]
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("Invalid l1_inclusion_block_number"))?;

        Ok(Self {
            batch_id,
            l1_inclusion_block_number,
        })
    }
}

impl std::fmt::Display for BatchMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.batch_id, self.l1_inclusion_block_number)
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BatchProofRequest {
    pub batches: Vec<BatchMetadata>,
    pub aggregate: bool,
    pub proof_type: ProofType,

    pub network: String,
    pub l1_network: String,
    pub graffiti: B256,
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,
    pub blob_proof_type: BlobProofType,
    #[serde(flatten)]
    pub prover_args: ProverSpecificOpts,
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BatchProofRequestOpt {
    // Required fields
    pub batches: Vec<BatchMetadata>,
    pub aggregate: Option<bool>,
    pub proof_type: String,

    // Optional fields, if not provided, the default values will be used
    pub network: Option<String>,
    pub l1_network: Option<String>,
    pub graffiti: Option<String>,
    pub prover: Option<String>,
    pub blob_proof_type: Option<String>,
    #[serde(flatten)]
    pub prover_args: Option<ProverSpecificOpts>,
}

impl TryFrom<BatchProofRequestOpt> for BatchProofRequest {
    type Error = RaikoError;

    fn try_from(value: BatchProofRequestOpt) -> Result<Self, Self::Error> {
        Ok(Self {
            batches: value.batches,
            aggregate: value.aggregate.unwrap_or(false),
            proof_type: value
                .proof_type
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid proof_type".to_string()))?,

            network: value.network.ok_or(RaikoError::InvalidRequestConfig(
                "Missing network".to_string(),
            ))?,
            l1_network: value.l1_network.ok_or(RaikoError::InvalidRequestConfig(
                "Missing l1_network".to_string(),
            ))?,
            graffiti: value
                .graffiti
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing graffiti".to_string(),
                ))?
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid graffiti".to_string()))?,
            prover: value
                .prover
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing prover".to_string(),
                ))?
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid prover".to_string()))?,
            blob_proof_type: value
                .blob_proof_type
                .unwrap_or("proof_of_equivalence".to_string())
                .parse()
                .map_err(|_| {
                    RaikoError::InvalidRequestConfig("Invalid blob_proof_type".to_string())
                })?,
            prover_args: value
                .prover_args
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing prover_args".to_string(),
                ))?
                .into(),
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ShastaProposalCheckpoint {
    pub block_number: u64,
    pub block_hash: B256,
    pub state_root: B256,
}

impl From<ShastaProposalCheckpoint> for Checkpoint {
    fn from(value: ShastaProposalCheckpoint) -> Self {
        Checkpoint {
            blockNumber: value.block_number.into(),
            blockHash: value.block_hash,
            stateRoot: value.state_root,
        }
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ShastaProposal {
    pub proposal_id: u64,
    pub checkpoint: Option<ShastaProposalCheckpoint>,
    pub l1_inclusion_block_number: u64,
    pub l2_block_numbers: Vec<u64>,
    pub last_anchor_block_number: u64,
}

impl std::fmt::Display for ShastaProposal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{:?}:{}",
            self.proposal_id,
            self.checkpoint,
            self.l1_inclusion_block_number
        )
    }
}
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ShastaProofRequest {
    pub proposals: Vec<ShastaProposal>,
    pub aggregate: bool,
    pub proof_type: ProofType,

    pub network: String,
    pub l1_network: String,
    pub graffiti: B256,
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,
    pub blob_proof_type: BlobProofType,
    #[serde(flatten)]
    pub prover_args: ProverSpecificOpts,
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ShastaProofRequestOpt {
    // Required fields
    pub proposals: Vec<ShastaProposal>,
    pub aggregate: Option<bool>,
    pub proof_type: String,

    // Optional fields, if not provided, the default values will be used
    pub network: Option<String>,
    pub l1_network: Option<String>,
    pub graffiti: Option<String>,
    pub prover: Option<String>,
    pub blob_proof_type: Option<String>,
    #[serde(flatten)]
    pub prover_args: Option<ProverSpecificOpts>,
}

impl TryFrom<ShastaProofRequestOpt> for ShastaProofRequest {
    type Error = RaikoError;

    fn try_from(value: ShastaProofRequestOpt) -> Result<Self, Self::Error> {
        Ok(Self {
            proposals: value.proposals,
            aggregate: value.aggregate.unwrap_or(false),
            proof_type: value
                .proof_type
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid proof_type".to_string()))?,

            network: value.network.ok_or(RaikoError::InvalidRequestConfig(
                "Missing network".to_string(),
            ))?,
            l1_network: value.l1_network.ok_or(RaikoError::InvalidRequestConfig(
                "Missing l1_network".to_string(),
            ))?,
            graffiti: value
                .graffiti
                .map_or_else(|| B256::ZERO, |s| s.parse().unwrap_or(B256::ZERO)),
            prover: value
                .prover
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing prover".to_string(),
                ))?
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid prover".to_string()))?,
            blob_proof_type: value
                .blob_proof_type
                .unwrap_or("proof_of_equivalence".to_string())
                .parse()
                .map_err(|_| {
                    RaikoError::InvalidRequestConfig("Invalid blob_proof_type".to_string())
                })?,
            prover_args: value
                .prover_args
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing prover_args".to_string(),
                ))?
                .into(),
        })
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, Args, PartialEq, Eq, Hash)]
pub struct ProverSpecificOpts {
    /// Native prover specific options.
    pub native: Option<Value>,
    /// SGX prover specific options.
    pub sgx: Option<Value>,
    /// SgxGeth prover specific options.
    pub sgxgeth: Option<Value>,
    /// SP1 prover specific options.
    pub sp1: Option<Value>,
    /// RISC0 prover specific options.
    pub risc0: Option<Value>,
}

impl<S: ::std::hash::BuildHasher + ::std::default::Default> From<ProverSpecificOpts>
    for HashMap<String, Value, S>
{
    fn from(value: ProverSpecificOpts) -> Self {
        [
            ("native", value.native.clone()),
            ("sgx", value.sgx.clone()),
            ("sgxgeth", value.sgxgeth.clone()),
            ("sp1", value.sp1.clone()),
            ("risc0", value.risc0.clone()),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|v| (name.to_string(), v)))
        .collect()
    }
}

impl ProofRequestOpt {
    /// Read a partial proof request config from a file.
    pub fn from_file<T>(path: T) -> RaikoResult<Self>
    where
        T: AsRef<Path>,
    {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let config: Value = serde_json::from_reader(reader)?;
        Self::deserialize(&config).map_err(|e| e.into())
    }

    /// Merge a partial proof request into current one.
    pub fn merge(&mut self, other: &Value) -> RaikoResult<()> {
        let mut this = serde_json::to_value(&self)?;
        merge(&mut this, other);
        *self = serde_json::from_value(this)?;
        Ok(())
    }
}

impl TryFrom<ProofRequestOpt> for ProofRequest {
    type Error = RaikoError;

    fn try_from(value: ProofRequestOpt) -> Result<Self, Self::Error> {
        Ok(Self {
            block_number: value.block_number.unwrap_or_default(),
            batch_id: value.batch_id.unwrap_or_default(),
            l1_inclusion_block_number: value.l1_inclusion_block_number.unwrap_or_default(),
            network: value.network.ok_or(RaikoError::InvalidRequestConfig(
                "Missing network".to_string(),
            ))?,
            l2_block_numbers: value.l2_block_numbers.unwrap_or_default(),
            l1_network: value.l1_network.ok_or(RaikoError::InvalidRequestConfig(
                "Missing l1_network".to_string(),
            ))?,
            graffiti: value
                .graffiti
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing graffiti".to_string(),
                ))?
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid graffiti".to_string()))?,
            prover: value
                .prover
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing prover".to_string(),
                ))?
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid prover".to_string()))?,
            proof_type: value
                .proof_type
                .ok_or(RaikoError::InvalidRequestConfig(
                    "Missing proof_type".to_string(),
                ))?
                .parse()
                .map_err(|_| RaikoError::InvalidRequestConfig("Invalid proof_type".to_string()))?,
            blob_proof_type: value
                .blob_proof_type
                .unwrap_or("proof_of_equivalence".to_string())
                .parse()
                .map_err(|_| {
                    RaikoError::InvalidRequestConfig("Invalid blob_proof_type".to_string())
                })?,
            prover_args: value.prover_args.into(),
            checkpoint: None,
            cached_event_data: None,
            last_anchor_block_number: None,
        })
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, Args)]
#[serde(default)]
/// A partial proof request config.
pub struct ProofRequestOpt {
    #[arg(long, require_equals = true)]
    /// The block number for the block to generate a proof for.
    pub block_number: Option<u64>,
    #[arg(long, require_equals = true)]
    /// The batch id for the batch of blocks to generate a proof for.
    pub batch_id: Option<u64>,
    #[arg(long, require_equals = true)]
    /// The block number for the l2 block to be proposed.
    /// in hekla, it is the anchored l1 block height - 1
    /// in ontake, it is the anchored l1 block height - (1..64)
    /// both above can be optional because raiko know anchor block id.
    /// in pacaya, it is the height of the l1 block which proposed the l2 block and must be presented
    /// as raiko does not know the anchor block id.
    pub l1_inclusion_block_number: Option<u64>,
    /// To support batch proof generation.
    /// The block numbers and l1 inclusion block numbers for the blocks to aggregate proofs for.
    /// This is used for batch proof generation.
    pub l2_block_numbers: Option<Vec<u64>>,
    #[arg(long, require_equals = true)]
    /// The network to generate the proof for.
    pub network: Option<String>,
    #[arg(long, require_equals = true)]
    /// The L1 network to generate the proof for.
    pub l1_network: Option<String>,
    #[arg(long, require_equals = true)]
    // Graffiti.
    pub graffiti: Option<String>,
    #[arg(long, require_equals = true)]
    /// The protocol instance data.
    pub prover: Option<String>,
    #[arg(long, require_equals = true)]
    /// The proof type.
    pub proof_type: Option<String>,
    /// Blob proof type.
    pub blob_proof_type: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    /// Any additional prover params in JSON format.
    pub prover_args: ProverSpecificOpts,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, PartialEq, Eq, Hash)]
#[serde(default)]
/// A request for proof aggregation of multiple proofs.
pub struct AggregationRequest {
    /// The block numbers and l1 inclusion block numbers for the blocks to aggregate proofs for.
    pub block_numbers: Vec<(u64, Option<u64>)>,
    /// The network to generate the proof for.
    pub network: Option<String>,
    /// The L1 network to generate the proof for.
    pub l1_network: Option<String>,
    // Graffiti.
    pub graffiti: Option<String>,
    /// The protocol instance data.
    pub prover: Option<String>,
    /// The proof type.
    pub proof_type: Option<String>,
    /// Blob proof type.
    pub blob_proof_type: Option<String>,
    #[serde(flatten)]
    /// Any additional prover params in JSON format.
    pub prover_args: ProverSpecificOpts,
}

impl AggregationRequest {
    /// Merge proof request options into aggregation request options.
    pub fn merge(&mut self, opts: &ProofRequestOpt) -> RaikoResult<()> {
        let this = serde_json::to_value(&self)?;
        let mut opts = serde_json::to_value(opts)?;
        merge(&mut opts, &this);
        *self = serde_json::from_value(opts)?;
        Ok(())
    }
}

impl From<AggregationRequest> for Vec<ProofRequestOpt> {
    fn from(value: AggregationRequest) -> Self {
        value
            .block_numbers
            .iter()
            .map(
                |&(block_number, l1_inclusion_block_number)| ProofRequestOpt {
                    block_number: Some(block_number),
                    batch_id: None,
                    l1_inclusion_block_number,
                    l2_block_numbers: None,
                    network: value.network.clone(),
                    l1_network: value.l1_network.clone(),
                    graffiti: value.graffiti.clone(),
                    prover: value.prover.clone(),
                    proof_type: value.proof_type.clone(),
                    blob_proof_type: value.blob_proof_type.clone(),
                    prover_args: value.prover_args.clone(),
                },
            )
            .collect()
    }
}

impl From<AggregationRequest> for ProofRequestOpt {
    fn from(value: AggregationRequest) -> Self {
        ProofRequestOpt {
            block_number: None,
            batch_id: None,
            l1_inclusion_block_number: None,
            l2_block_numbers: None,
            network: value.network,
            l1_network: value.l1_network,
            graffiti: value.graffiti,
            prover: value.prover,
            proof_type: value.proof_type,
            blob_proof_type: value.blob_proof_type,
            prover_args: value.prover_args,
        }
    }
}

impl From<ProofRequestOpt> for AggregationRequest {
    fn from(value: ProofRequestOpt) -> Self {
        let block_numbers = if let Some(block_number) = value.block_number {
            vec![(block_number, value.l1_inclusion_block_number)]
        } else {
            vec![]
        };

        Self {
            block_numbers,
            network: value.network,
            l1_network: value.l1_network,
            graffiti: value.graffiti,
            prover: value.prover,
            proof_type: value.proof_type,
            blob_proof_type: value.blob_proof_type,
            prover_args: value.prover_args,
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, PartialEq)]
#[serde(default)]
/// A request for proof aggregation of multiple proofs.
pub struct AggregationOnlyRequest {
    /// The block numbers and l1 inclusion block numbers for the blocks to aggregate proofs for.
    pub aggregation_ids: Vec<u64>,
    /// The block numbers and l1 inclusion block numbers for the blocks to aggregate proofs for.
    pub proofs: Vec<Proof>,
    /// The proof type.
    pub proof_type: Option<String>,
    #[serde(flatten)]
    /// Any additional prover params in JSON format.
    pub prover_args: ProverSpecificOpts,
}

impl Display for AggregationOnlyRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "AggregationOnlyRequest {{{:?}, {:?}}}",
            self.aggregation_ids, self.proof_type
        ))
    }
}

impl From<(AggregationRequest, Vec<Proof>)> for AggregationOnlyRequest {
    fn from((request, proofs): (AggregationRequest, Vec<Proof>)) -> Self {
        Self {
            proofs,
            aggregation_ids: request.block_numbers.iter().map(|(id, _)| *id).collect(),
            proof_type: request.proof_type,
            prover_args: request.prover_args,
        }
    }
}

impl AggregationOnlyRequest {
    /// Merge proof request options into aggregation request options.
    pub fn merge(&mut self, opts: &ProofRequestOpt) -> RaikoResult<()> {
        let this = serde_json::to_value(&self)?;
        let mut opts = serde_json::to_value(opts)?;
        merge(&mut opts, &this);
        *self = serde_json::from_value(opts)?;
        Ok(())
    }
}
