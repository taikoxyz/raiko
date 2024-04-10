use core::fmt::Debug;
use std::{path::Path, str::FromStr};

use alloy_consensus::Sealable;
use alloy_primitives::{Address, B256};
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    consts::Network,
    input::{GuestInput, GuestOutput, WrappedHeader},
    protocol_instance::{assemble_protocol_instance, ProtocolInstance},
    prover::{Proof, Prover},
    Measurement,
};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use structopt::StructOpt;
use tracing::{info, warn};

use crate::{
    error::{HostError, HostResult},
    execution::{prepare_input, NativeDriver},
    memory,
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Deserialize, Serialize)]
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
            _ => Err(HostError::InvlaidProofType(s.to_string())),
        }
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
/// A request for a proof.
pub struct ProofRequest {
    /// The block number for the block to generate a proof for.
    pub block_number: u64,
    /// RPC URL for retreiving block by block number.
    pub rpc: String,
    /// The L1 node URL for signal root verify and get txlist info from proposed
    /// transaction.
    pub l1_rpc: String,
    /// The beacon node URL for retreiving data blobs.
    pub beacon_rpc: String,
    /// The network to generate the proof for.
    pub network: Network,
    /// L1 network selection
    pub l1_network: String,
    /// Graffiti.
    pub graffiti: B256,
    /// The protocol instance data.
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,
    /// The proof type.
    pub proof_type: ProofType,
}

#[derive(StructOpt, Default, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
/// A partial proof request config.
pub struct ProofRequestOpt {
    #[structopt(long, require_equals = true)]
    /// The block number for the block to generate a proof for.
    pub block_number: Option<u64>,
    #[structopt(long, require_equals = true)]
    /// RPC URL for retreiving block by block number.
    pub rpc: Option<String>,
    #[structopt(long, require_equals = true)]
    /// The L1 node URL for signal root verify and get txlist info from proposed
    /// transaction.
    pub l1_rpc: Option<String>,
    #[structopt(long, require_equals = true)]
    /// The beacon node URL for retreiving data blobs.
    pub beacon_rpc: Option<String>,
    #[structopt(long, require_equals = true)]
    /// The network to generate the proof for.
    pub network: Option<String>,
    #[structopt(long, require_equals = true)]
    // Graffiti.
    pub graffiti: Option<String>,
    #[structopt(long, require_equals = true)]
    /// The protocol instance data.
    pub prover: Option<String>,
    #[structopt(long, require_equals = true)]
    /// The proof type.
    pub proof_type: Option<String>,
}

impl ProofRequestOpt {
    /// Read a partial proof request config from a file.
    pub fn from_file<T>(path: T) -> Result<Self, HostError>
    where
        T: AsRef<Path>,
    {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let config: serde_json::Value = serde_json::from_reader(reader)?;
        Self::deserialize(&config).map_err(|e| e.into())
    }

    /// Merge a partial proof request into current one.
    pub fn merge(&mut self, other: &Self) {
        if other.block_number.is_some() {
            self.block_number = other.block_number;
        }
        if other.rpc.is_some() {
            self.rpc.clone_from(&other.rpc);
        }
        if other.l1_rpc.is_some() {
            self.l1_rpc.clone_from(&other.l1_rpc);
        }
        if other.beacon_rpc.is_some() {
            self.beacon_rpc.clone_from(&other.beacon_rpc);
        }
        if other.network.is_some() {
            self.network.clone_from(&other.network);
        }
        if other.graffiti.is_some() {
            self.graffiti.clone_from(&other.graffiti);
        }
        if other.prover.is_some() {
            self.prover.clone_from(&other.prover);
        }
        if other.proof_type.is_some() {
            self.proof_type.clone_from(&other.proof_type);
        }
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
        })
    }
}

impl ProofRequest {
    /// Get the instance hash for the protocol instance depending on the proof type.
    fn instance_hash(&self, pi: ProtocolInstance) -> HostResult<B256> {
        match self.proof_type {
            ProofType::Native => Ok(NativeDriver::instance_hash(pi)),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return Ok((sp1_prover::Sp1Prover::instance_hash(pi)));

                Err(HostError::FeatureNotSupportedError(self.proof_type.clone()))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return Ok(risc0_prover::Risc0Prover::instance_hash(pi));

                Err(HostError::FeatureNotSupportedError(self.proof_type.clone()))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return Ok(sgx_prover::SgxProver::instance_hash(pi));

                Err(HostError::FeatureNotSupportedError(self.proof_type.clone()))
            }
        }
    }

    /// Run the prover driver depending on the proof type.
    async fn run_driver(
        &self,
        input: GuestInput,
        output: GuestOutput,
        config: &serde_json::Value,
    ) -> HostResult<Proof> {
        match self.proof_type {
            ProofType::Native => NativeDriver::run(input, output, config)
                .await
                .map_err(|e| e.into()),
            ProofType::Sp1 => {
                #[cfg(feature = "sp1")]
                return sp1_prover::Sp1Prover::run(input, output, config)
                    .await
                    .map_err(|e| e.into());

                Err(HostError::FeatureNotSupportedError(self.proof_type.clone()))
            }
            ProofType::Risc0 => {
                #[cfg(feature = "risc0")]
                return risc0_prover::Risc0Prover::run(input, output, config)
                    .await
                    .map_err(|e| e.into());

                Err(HostError::FeatureNotSupportedError(self.proof_type.clone()))
            }
            ProofType::Sgx => {
                #[cfg(feature = "sgx")]
                return sgx_prover::SgxProver::run(input, output, config)
                    .await
                    .map_err(|e| e.into());

                Err(HostError::FeatureNotSupportedError(self.proof_type.clone()))
            }
        }
    }

    /// Execute the proof generation.
    pub async fn execute(
        &self,
        cached_input: Option<GuestInput>,
    ) -> HostResult<(GuestInput, Proof)> {
        // 1. Prepare input - use cached input if available, otherwise prepare new input
        let input = if let Some(cached_input) = cached_input {
            println!("Using cached input");
            cached_input
        } else {
            memory::reset_stats();
            let measurement = Measurement::start("Generating input...", false);
            let input = prepare_input(self.clone()).await?;
            measurement.stop_with("=> Input generated");
            memory::print_stats("Input generation peak memory used: ");
            input
        };

        // 2. Test run the block
        memory::reset_stats();
        let build_result = TaikoStrategy::build_from(&input);
        let output = match &build_result {
            Ok((header, _mpt_node)) => {
                info!("Verifying final state using provider data ...");
                info!("Final block hash derived successfully. {}", header.hash());
                info!("Final block header derived successfully. {header:?}");
                let pi = self.instance_hash(assemble_protocol_instance(&input, header)?)?;
                // Make sure the blockhash from the node matches the one from the builder
                assert_eq!(header.hash().0, input.block_hash, "block hash unexpected");
                GuestOutput::Success((
                    WrappedHeader {
                        header: header.clone(),
                    },
                    pi,
                ))
            }
            Err(_) => {
                warn!("Proving bad block construction!");
                GuestOutput::Failure
            }
        };
        memory::print_stats("Guest program peak memory used: ");

        // 3. Prove
        memory::reset_stats();
        let measurement = Measurement::start("Generating proof...", false);
        let res = self
            .run_driver(input.clone(), output, &serde_json::to_value(self)?)
            .await
            .map(|proof| (input, proof));
        measurement.stop_with("=> Proof generated");
        memory::print_stats("Prover peak memory used: ");

        res
    }
}
