use core::fmt::Debug;
use std::collections::HashMap;
use std::{path::Path, str::FromStr};

use alloy_primitives::{Address, B256};
use clap::{Args, ValueEnum};
use raiko_lib::{
    consts::Network,
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{Proof, Prover},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{serde_as, DisplayFromStr};
use utoipa::ToSchema;

use crate::{
    error::{HostError, HostResult},
    merge,
    raiko::NativeProver,
};

#[derive(
    PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Deserialize, Serialize, ToSchema, Hash, ValueEnum,
)]
/// Available proof types.
pub enum ProofType {
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
}

impl std::fmt::Display for ProofType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ProofType::Native => "native",
            ProofType::Sp1 => "sp1",
            ProofType::Sgx => "sgx",
            ProofType::Risc0 => "risc0",
        })
    }
}

impl FromStr for ProofType {
    type Err = HostError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "native" => Ok(ProofType::Native),
            "sp1" => Ok(ProofType::Sp1),
            "sgx" => Ok(ProofType::Sgx),
            "risc0" => Ok(ProofType::Risc0),
            _ => Err(HostError::InvalidProofType(s.to_string())),
        }
    }
}

impl ProofType {
    /// Get the instance hash for the protocol instance depending on the proof type.
    pub fn instance_hash(&self, pi: ProtocolInstance) -> HostResult<B256> {
        match self {
            ProofType::Native => Ok(NativeProver::instance_hash(pi)),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return Ok(sp1_driver::Sp1Prover::instance_hash(pi));

                Err(HostError::FeatureNotSupportedError(self.clone()))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return Ok(risc0_driver::Risc0Prover::instance_hash(pi));

                Err(HostError::FeatureNotSupportedError(self.clone()))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return Ok(sgx_prover::SgxProver::instance_hash(pi));

                Err(HostError::FeatureNotSupportedError(self.clone()))
            }
        }
    }

    /// Run the prover driver depending on the proof type.
    pub async fn run_prover(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &Value,
    ) -> HostResult<Proof> {
        match self {
            ProofType::Native => NativeProver::run(input, output, config)
                .await
                .map_err(|e| e.into()),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return sp1_driver::Sp1Prover::run(input, output, config)
                    .await
                    .map_err(|e| e.into());

                Err(HostError::FeatureNotSupportedError(self.clone()))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return risc0_driver::Risc0Prover::run(input, output, config)
                    .await
                    .map_err(|e| e.into());

                Err(HostError::FeatureNotSupportedError(self.clone()))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return sgx_prover::SgxProver::run(input, output, config)
                    .await
                    .map_err(|e| e.into());

                Err(HostError::FeatureNotSupportedError(self.clone()))
            }
        }
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
/// A request for a proof.
pub struct ProofRequest {
    /// The block number for the block to generate a proof for.
    pub block_number: u64,
    /// RPC URL for retrieving block by block number.
    pub rpc: String,
    /// The L1 node URL for signal root verify and get txlist info from proposed
    /// transaction.
    pub l1_rpc: String,
    /// The beacon node URL for retrieving data blobs.
    pub beacon_rpc: String,
    /// The network to generate the proof for.
    pub network: Network,
    /// The L1 network to grnerate the proof for.
    pub l1_network: String,
    /// Graffiti.
    pub graffiti: B256,
    /// The protocol instance data.
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,
    /// The proof type.
    pub proof_type: ProofType,
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
    /// RPC URL for retrieving block by block number.
    pub rpc: Option<String>,
    #[arg(long, require_equals = true)]
    /// The L1 node URL for signal root verify and get txlist info from proposed
    /// transaction.
    pub l1_rpc: Option<String>,
    #[arg(long, require_equals = true)]
    /// The beacon node URL for retrieving data blobs.
    pub beacon_rpc: Option<String>,
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
    pub fn from_file<T>(path: T) -> HostResult<Self>
    where
        T: AsRef<Path>,
    {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let config: Value = serde_json::from_reader(reader)?;
        Self::deserialize(&config).map_err(|e| e.into())
    }

    /// Merge a partial proof request into current one.
    pub fn merge(&mut self, other: &Value) -> HostResult<()> {
        let mut this = serde_json::to_value(&self)?;
        merge(&mut this, other);
        *self = serde_json::from_value(this)?;
        Ok(())
    }
}

impl TryFrom<ProofRequestOpt> for ProofRequest {
    type Error = HostError;

    fn try_from(value: ProofRequestOpt) -> Result<Self, Self::Error> {
        Ok(Self {
            block_number: value.block_number.ok_or(HostError::InvalidRequestConfig(
                "Missing block number".to_string(),
            ))?,
            rpc: value
                .rpc
                .ok_or(HostError::InvalidRequestConfig("Missing rpc".to_string()))?,
            l1_rpc: value.l1_rpc.ok_or(HostError::InvalidRequestConfig(
                "Missing l1_rpc".to_string(),
            ))?,
            beacon_rpc: value.beacon_rpc.ok_or(HostError::InvalidRequestConfig(
                "Missing beacon_rpc".to_string(),
            ))?,
            network: value
                .network
                .ok_or(HostError::InvalidRequestConfig(
                    "Missing network".to_string(),
                ))?
                .parse()
                .map_err(|_| HostError::InvalidRequestConfig("Invalid network".to_string()))?,
            l1_network: value.l1_network.ok_or(HostError::InvalidRequestConfig(
                "Missing l1_network".to_string(),
            ))?,
            graffiti: value
                .graffiti
                .ok_or(HostError::InvalidRequestConfig(
                    "Missing graffiti".to_string(),
                ))?
                .parse()
                .map_err(|_| HostError::InvalidRequestConfig("Invalid graffiti".to_string()))?,
            prover: value
                .prover
                .ok_or(HostError::InvalidRequestConfig(
                    "Missing prover".to_string(),
                ))?
                .parse()
                .map_err(|_| HostError::InvalidRequestConfig("Invalid prover".to_string()))?,
            proof_type: value
                .proof_type
                .ok_or(HostError::InvalidRequestConfig(
                    "Missing proof_type".to_string(),
                ))?
                .parse()
                .map_err(|_| HostError::InvalidRequestConfig("Invalid proof_type".to_string()))?,
            prover_args: value.prover_args.into(),
        })
    }
}
