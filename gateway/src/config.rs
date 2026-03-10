use crate::ShastaRouteDefaults;
use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    pub backend: BackendConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfig {
    /// URLs for consistency-hashed routing (one per backend replica)
    pub urls: Vec<String>,
    /// URL for passthrough (list, query, etc.). Defaults to urls[0] when omitted (single-replica).
    pub shared_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub l1_network: String,
    #[serde(default = "default_proof_type")]
    pub proof_type: String,
    #[serde(default = "default_prover")]
    pub prover: String,
    #[serde(default)]
    pub aggregate: bool,
}

fn default_proof_type() -> String {
    "native".to_string()
}

fn default_prover() -> String {
    "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string()
}

#[derive(Debug, Clone, Parser)]
pub struct Cli {
    /// Path to config file (toml)
    #[arg(short, long)]
    pub config: PathBuf,
}

impl Config {
    pub fn load(path: &PathBuf) -> Result<Self> {
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config from {}", path.display()))?;
        let config: Config =
            toml::from_str(&s).with_context(|| format!("failed to parse config from {}", path.display()))?;
        if config.backend.urls.is_empty() {
            anyhow::bail!("backend.urls must not be empty");
        }
        Ok(config)
    }

    pub fn backend_url(&self, index: usize) -> Option<&str> {
        self.backend.urls.get(index).map(String::as_str)
    }

    pub fn shared_backend_url(&self) -> &str {
        self.backend
            .shared_url
            .as_deref()
            .unwrap_or_else(|| self.backend.urls.first().map(String::as_str).unwrap_or(""))
    }

    pub fn backend_replicas(&self) -> usize {
        self.backend.urls.len()
    }

    pub fn route_defaults(&self) -> ShastaRouteDefaults {
        ShastaRouteDefaults {
            l1_network: self.defaults.l1_network.clone(),
            network: self.defaults.network.clone(),
            proof_type: self.defaults.proof_type.clone(),
            prover: self.defaults.prover.clone(),
            aggregate: self.defaults.aggregate,
        }
    }
}
