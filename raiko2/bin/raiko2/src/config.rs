//! Configuration management for Raiko V2.

use crate::cli::Cli;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
        }
    }
}

/// RPC configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub l1_rpc: String,
    pub l2_rpc: String,
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            l1_rpc: "http://localhost:8545".to_string(),
            l2_rpc: "http://localhost:9545".to_string(),
            l1_chain_id: 1,
            l2_chain_id: 167000,
        }
    }
}

/// Prover type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProverType {
    #[default]
    Risc0,
    Sp1,
}

impl std::str::FromStr for ProverType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "risc0" => Ok(ProverType::Risc0),
            "sp1" => Ok(ProverType::Sp1),
            _ => Err(format!("Unknown prover type: {}", s)),
        }
    }
}

/// Prover configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProverConfig {
    pub prover_type: ProverType,
    /// RISC0 specific configuration.
    #[serde(default)]
    pub risc0: Risc0Config,
    /// SP1 specific configuration.
    #[serde(default)]
    pub sp1: Sp1Config,
}

/// RISC0 configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Risc0Config {
    pub bonsai: bool,
    pub snark: bool,
}

impl Default for Risc0Config {
    fn default() -> Self {
        Self {
            bonsai: true,
            snark: true,
        }
    }
}

/// SP1 configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sp1Config {
    pub network: bool,
    pub plonk: bool,
}

impl Default for Sp1Config {
    fn default() -> Self {
        Self {
            network: true,
            plonk: true,
        }
    }
}

/// Full application configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub server: ServerConfig,
    pub rpc: RpcConfig,
    pub prover: ProverConfig,
}

impl Config {
    /// Load configuration from CLI arguments and optional config file.
    pub fn load(cli: &Cli) -> Result<Self> {
        let mut config = if let Some(config_path) = &cli.config {
            Self::from_file(config_path)?
        } else {
            Self::default()
        };

        // Override with CLI arguments
        config.server.host = cli.host.clone();
        config.server.port = cli.port;

        if let Some(l1_rpc) = &cli.l1_rpc {
            config.rpc.l1_rpc = l1_rpc.clone();
        }
        if let Some(l2_rpc) = &cli.l2_rpc {
            config.rpc.l2_rpc = l2_rpc.clone();
        }
        config.rpc.l1_chain_id = cli.l1_chain_id;
        config.rpc.l2_chain_id = cli.l2_chain_id;

        config.prover.prover_type = cli.prover.parse().map_err(|e: String| anyhow::anyhow!(e))?;

        Ok(config)
    }

    /// Load configuration from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;
        toml::from_str(&content).with_context(|| format!("Failed to parse config file: {:?}", path))
    }
}
