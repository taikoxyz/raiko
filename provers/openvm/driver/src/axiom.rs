// Axiom API integration for remote OpenVM proving
// This module will be implemented when Axiom API key is available

use crate::OpenVMResponse;
use raiko_lib::prover::{IdWrite, ProofKey, ProverError, ProverResult};

/// Prove using Axiom network (placeholder - requires API key)
pub async fn prove_axiom(
    _elf: &[u8],
    _input: &[u8],
    _proof_key: ProofKey,
    _id_store: Option<&mut dyn IdWrite>,
) -> ProverResult<OpenVMResponse> {
    Err(ProverError::GuestError(
        "Axiom API not yet implemented - apply for API key first".to_string(),
    ))
}

/// Cancel a proof request on Axiom network
pub async fn cancel_proof(_uuid: String) -> ProverResult<()> {
    Err(ProverError::GuestError(
        "Axiom API not yet implemented".to_string(),
    ))
}

// TODO: Implement when Axiom API key is available
// Reference: https://docs.axiom.xyz/api-reference/sdks/rust-client-sdk
//
// Implementation should include:
// 1. Authentication with API key (from env: AXIOM_API_KEY)
// 2. Upload ELF program
// 3. Submit proof request with input
// 4. Poll for proof completion
// 5. Download and return proof
// 6. Handle errors and retries
