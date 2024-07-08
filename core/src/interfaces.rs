use crate::{merge, prover::NativeProver};
use alloy_primitives::{Address, B256};
use clap::{Args, ValueEnum};
use raiko_lib::{
    input::{BlobProofType, GuestInput, GuestOutput},
    primitives::eip4844::{calc_kzg_proof, commitment_to_version_hash, kzg_proof_to_bytes},
    prover::{Proof, Prover, ProverError},
};
use reth_primitives::hex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{serde_as, DisplayFromStr};
use std::{collections::HashMap, path::Path, str::FromStr};
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
    /// # Nitro
    ///
    /// Uses Nitro enclave prover.
    Nitro,
}

impl std::fmt::Display for ProofType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ProofType::Native => "native",
            ProofType::Sp1 => "sp1",
            ProofType::Sgx => "sgx",
            ProofType::Risc0 => "risc0",
            ProofType::Nitro => "nitro",
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
            "nitro" => Ok(ProofType::Nitro),
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
            _ => Err(RaikoError::Conversion("Invalid u8".to_owned())),
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
    ) -> RaikoResult<Proof> {
        let mut proof = match self {
            ProofType::Native => NativeProver::run(input.clone(), output, config)
                .await
                .map_err(<ProverError as Into<RaikoError>>::into),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return sp1_driver::Sp1Prover::run(input.clone(), output, config)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sp1"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return risc0_driver::Risc0Prover::run(input.clone(), output, config)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "risc0"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return sgx_prover::SgxProver::run(input.clone(), output, config)
                    .await
                    .map_err(|e| e.into());
                #[cfg(not(feature = "sgx"))]
                Err(RaikoError::FeatureNotSupportedError(*self))
            }
            ProofType::Nitro => {
                #[cfg(feature = "nitro")]
                return nitro_prover::NitroProver::prove(input).map_err(|e| e.into());
                #[cfg(not(feature = "nitro"))]
                Err(RaikoError::FeatureNotSupportedError(self))
            }
        }?;

        // Add the kzg proof to the proof if needed
        if let Some(blob_commitment) = input.taiko.blob_commitment.clone() {
            let kzg_proof = calc_kzg_proof(
                &input.taiko.tx_data,
                &commitment_to_version_hash(&blob_commitment.try_into().unwrap()),
            )
            .unwrap();
            let kzg_proof_hex = hex::encode(kzg_proof_to_bytes(&kzg_proof));
            proof
                .as_object_mut()
                .unwrap()
                .insert("kzg_proof".to_string(), Value::String(kzg_proof_hex));
        }

        Ok(proof)
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
/// A request for a proof.
pub struct ProofRequest {
    /// The block number for the block to generate a proof for.
    pub block_number: u64,
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

#[derive(Default, Clone, Serialize, Deserialize, Debug, ToSchema, Args)]
pub struct ProverSpecificOpts {
    /// Native prover specific options.
    pub native: Option<Value>,
    /// SGX prover specific options.
    pub sgx: Option<Value>,
    /// SP1 prover specific options.
    pub sp1: Option<Value>,
    /// RISC0 prover specific options.
    pub risc0: Option<Value>,
    /// Nitro enclave specific options.
    pub nitro: Option<Value>,
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
            ("nitro", value.nitro.clone()),
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
                .unwrap_or("ProofOfEquivalence".to_string())
                .parse()
                .map_err(|_| {
                    RaikoError::InvalidRequestConfig("Invalid blob_proof_type".to_string())
                })?,
            prover_args: value.prover_args.into(),
        })
    }
}
