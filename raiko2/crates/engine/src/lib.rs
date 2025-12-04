//! Raiko2 Engine - core proving engine for batch proofs.
//!
//! This module provides the main `Engine` type that orchestrates:
//! - Input creation from RPC providers
//! - Local validation of inputs
//! - Proof generation using zkVM provers
//!
//! Note: Prover integration temporarily disabled due to version conflicts.
//! TaikoEvmConfig integration pending alethia-reth-block dependency.

use alloy_consensus::transaction::SignerRecoverable;
use raiko2_driver::Driver;
use raiko2_primitives::{
    GuestInput, GuestOutput, ProofContext, RaikoError, RaikoResult, StatelessInput,
};
use raiko2_provider::Provider;
use tracing::info;

/// Proof type enum for raiko2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofType {
    /// RISC0 zkVM proof
    Risc0,
    /// SP1 zkVM proof
    Sp1,
    /// Native execution (no proof)
    Native,
}

impl std::fmt::Display for ProofType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProofType::Risc0 => write!(f, "risc0"),
            ProofType::Sp1 => write!(f, "sp1"),
            ProofType::Native => write!(f, "native"),
        }
    }
}

impl std::str::FromStr for ProofType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "risc0" => Ok(ProofType::Risc0),
            "sp1" => Ok(ProofType::Sp1),
            "native" => Ok(ProofType::Native),
            _ => Err(format!("Unknown proof type: {}", s)),
        }
    }
}

/// The main proving engine.
#[derive(Debug)]
pub struct Engine<P: Provider> {
    driver: Driver,
    provider: P,
}

impl<P: Provider> Engine<P> {
    /// Create a new engine with the given provider.
    pub fn new(driver: Driver, provider: P) -> Self {
        Self { driver, provider }
    }

    /// Create guest input from the proof context.
    pub async fn create_input(&self, ctx: &ProofContext) -> RaikoResult<GuestInput> {
        info!("Creating input for batch {}", ctx.request.batch_id);

        // Get block numbers from the request
        let block_numbers: Vec<u64> = vec![ctx.request.batch_id]; // Simplified for now

        let blocks = self.provider.batch_blocks(&block_numbers).await?;
        let taiko_manifest = self.driver.taiko_manifest(ctx, &blocks).await?;
        let witnesses = self.provider.batch_witnesses(&block_numbers).await?;

        // Collect signers for each block
        let mut all_signers = Vec::with_capacity(blocks.len());
        for block in blocks.iter() {
            let mut signers = Vec::new();
            for tx in block.body.transactions() {
                if let Ok(signer) = tx.recover_signer() {
                    signers.push(signer);
                }
            }
            all_signers.push(signers);
        }

        let accounts = self
            .provider
            .batch_accounts(&block_numbers, &all_signers)
            .await?;

        Ok(GuestInput {
            taiko: taiko_manifest,
            witnesses: blocks
                .into_iter()
                .zip(witnesses)
                .zip(accounts)
                .map(|((block, witness), accounts)| StatelessInput {
                    block,
                    witness,
                    accounts,
                })
                .collect(),
        })
    }

    /// Generate output from the input (for verification).
    pub fn generate_output(&self, input: &GuestInput) -> RaikoResult<GuestOutput> {
        let last_block = input
            .witnesses
            .last()
            .ok_or_else(|| RaikoError::InvalidRequestConfig("No blocks in input".to_string()))?;

        Ok(GuestOutput {
            header: last_block.block.header.clone(),
            hash: last_block.block.header.state_root,
        })
    }
}

