//! Raiko V2 - Taiko zkVM Prover Server
//!
//! This binary provides a REST API for generating zero-knowledge proofs
//! of Taiko block execution using RISC0 or SP1 zkVMs.
//!
//! ## Usage
//!
//! ```bash
//! # Start the server
//! raiko2 --config config.toml
//!
//! # Or with environment variables
//! RAIKO2_L1_RPC=http://localhost:8545 \
//! RAIKO2_L2_RPC=http://localhost:9545 \
//! raiko2
//! ```

mod cli;
mod config;
mod server;

use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::cli::Cli;
use crate::config::Config;
use crate::server::run_server;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present
    dotenvy::dotenv().ok();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Initialize logging
    init_logging(&cli)?;

    info!("Starting Raiko V2 Prover Server");

    // Load configuration
    let config = Config::load(&cli)?;
    info!("Loaded configuration: {:?}", config.server);

    // Run the server
    run_server(config).await?;

    Ok(())
}

fn init_logging(cli: &Cli) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            if cli.verbose {
                EnvFilter::new("debug")
            } else {
                EnvFilter::new("info")
            }
        });

    if cli.json_logs {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer())
            .init();
    }

    Ok(())
}
