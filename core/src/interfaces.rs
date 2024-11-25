use crate::{merge, prover::NativeProver};
use alloy_primitives::{Address, B256};
use clap::{Args, ValueEnum};
use raiko_lib::{
    consts::VerifierType,
    input::{
        AggregationGuestInput, AggregationGuestOutput, BlobProofType, GuestInput, GuestOutput,
    },
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverError},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{serde_as, DisplayFromStr};
use std::{collections::HashMap, fmt::Display, path::Path, str::FromStr};
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

#[derive(
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Clone,
    Debug,
    Default,
    Deserialize,
    Serialize,
    ToSchema,
    Hash,
    ValueEnum,
    Copy,
)]
/// Available proof types.
pub enum ProofType {
    #[default]
    /// # Native
    ///
    /// This builds the block the same way the node does and then runs the result.
    Native,
    /// # Sp1
    ///
    /// Uses the SP1 prover to build the block.
    Sp1,
    /// # Sgx
    ///
    /// Builds the block on a SGX supported CPU to create a proof.
    Sgx,
    /// # Risc0
    ///
    /// Uses the RISC0 prover to build the block.
    Risc0,
    /// # powdr
    ///
    /// Uses powdrVM to build the block.
    Powdr,
}

impl std::fmt::Display for ProofType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ProofType::Native => "native",
            ProofType::Sp1 => "sp1",
            ProofType::Sgx => "sgx",
            ProofType::Risc0 => "risc0",
            ProofType::Powdr => "powdr",
        })
    }
}

impl FromStr for ProofType {
    type Err = RaikoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "native" => Ok(ProofType::Native),
            "sp1" => Ok(ProofType::Sp1),
            "sgx" => Ok(ProofType::Sgx),
            "risc0" => Ok(ProofType::Risc0),
            "powdr" => Ok(ProofType::Powdr),
            _ => Err(RaikoError::InvalidProofType(s.to_string())),
        }
    }
}

impl TryFrom<u8> for ProofType {
    type Error = RaikoError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Native),
            1 => Ok(Self::Sp1),
            2 => Ok(Self::Sgx),
            3 => Ok(Self::Risc0),
            4 => Ok(Self::Powdr),
            _ => Err(RaikoError::Conversion("Invalid u8".to_owned())),
        }
    }
}

impl From<ProofType> for VerifierType {
    fn from(val: ProofType) -> Self {
        match val {
            ProofType::Native => VerifierType::None,
            ProofType::Sp1 => VerifierType::SP1,
            ProofType::Sgx => VerifierType::SGX,
            ProofType::Risc0 => VerifierType::RISC0,
            ProofType::Powdr => VerifierType::Powdr,
        }
    }
}

impl ProofType {
    /// Run the prover driver depending on the proof type.
    pub async fn run_prover(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &Value,
        store: Option<&mut dyn IdWrite>,
    ) -> RaikoResult<Proof> {
        match self {
            ProofType::Native => NativeProver::run(input.clone(), output, config, store)
                .await
                .map_err(<ProverError as Into<RaikoError>>::into),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return sp1_driver::Sp1Prover::run(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sp1"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return risc0_driver::Risc0Prover::run(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "risc0"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Powdr => {
                #[cfg(feature = "powdr")]
                return powdr_driver::PowdrProver::run(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "powdr"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return sgx_prover::SgxProver::run(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sgx"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
        }
    }

    /// Run the prover driver depending on the proof type.
    pub async fn aggregate_proofs(
        &self,
        input: AggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &Value,
        store: Option<&mut dyn IdWrite>,
    ) -> RaikoResult<Proof> {
        let proof = match self {
            ProofType::Native => NativeProver::aggregate(input.clone(), output, config, store)
                .await
                .map_err(<ProverError as Into<RaikoError>>::into),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return sp1_driver::Sp1Prover::aggregate(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sp1"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return risc0_driver::Risc0Prover::aggregate(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "risc0"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Powdr => {
                #[cfg(feature = "powdr")]
                return powdr_driver::PowdrProver::aggregate(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "powdr"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return sgx_prover::SgxProver::aggregate(input.clone(), output, config, store)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sgx"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
        }?;

        Ok(proof)
    }

    pub async fn cancel_proof(
        &self,
        proof_key: ProofKey,
        read: Box<&mut dyn IdStore>,
    ) -> RaikoResult<()> {
        match self {
            ProofType::Native => NativeProver::cancel(proof_key, read)
                .await
                .map_err(<ProverError as Into<RaikoError>>::into),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return sp1_driver::Sp1Prover::cancel(proof_key, read)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sp1"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return risc0_driver::Risc0Prover::cancel(proof_key, read)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "risc0"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Powdr => {
                #[cfg(feature = "powdr")]
                return powdr_driver::PowdrProver::cancel(proof_key, read)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "powdr"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return sgx_prover::SgxProver::cancel(proof_key, read)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sgx"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
        }?;
        Ok(())
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
/// A request for a proof.
pub struct ProofRequest {
    /// The block number for the block to generate a proof for.
    pub block_number: u64,
    /// The l1 block number of the l2 block be proposed.
    pub l1_inclusion_block_number: u64,
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
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, Args)]
#[serde(default)]
/// A partial proof request config.
pub struct ProofRequestOpt {
    #[arg(long, require_equals = true)]
    /// The block number for the block to generate a proof for.
    pub block_number: Option<u64>,
    #[arg(long, require_equals = true)]
    /// The block number for the l2 block to be proposed.
    /// in hekla, it is the anchored l1 block height - 1
    /// in ontake, it is the anchored l1 block height - (1..64)
    pub l1_inclusion_block_number: Option<u64>,
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

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, Args, PartialEq, Eq, Hash)]
pub struct ProverSpecificOpts {
    /// Native prover specific options.
    pub native: Option<Value>,
    /// SGX prover specific options.
    pub sgx: Option<Value>,
    /// SP1 prover specific options.
    pub sp1: Option<Value>,
    /// RISC0 prover specific options.
    pub risc0: Option<Value>,
    /// Powdr prover specific options.
    pub powdr: Option<Value>,
}

impl<S: ::std::hash::BuildHasher + ::std::default::Default> From<ProverSpecificOpts>
    for HashMap<String, Value, S>
{
    fn from(value: ProverSpecificOpts) -> Self {
        [
            ("native", value.native.clone()),
            ("sgx", value.sgx.clone()),
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
            block_number: value.block_number.ok_or(RaikoError::InvalidRequestConfig(
                "Missing block number".to_string(),
            ))?,
            l1_inclusion_block_number: value.l1_inclusion_block_number.unwrap_or_default(),
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
        })
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema)]
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
                    l1_inclusion_block_number,
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

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, PartialEq, Eq, Hash)]
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
            "AggregationOnlyRequest {{ {:?}, {:?} }}",
            self.proof_type, self.prover_args
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
