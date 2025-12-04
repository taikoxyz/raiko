//! Proof context for raiko2.

use std::sync::Arc;

use crate::proof::ProverConfig;
use alethia_reth_node::chainspec::spec::TaikoChainSpec;
use reth::chainspec::ChainSpec as RethChainSpec;
use serde::{Deserialize, Serialize};

/// Proof request parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofRequest {
    /// The L1 chain ID.
    pub l1_chain_id: u64,
    /// The L2 chain ID.
    pub l2_chain_id: u64,
    /// The batch ID to prove.
    pub batch_id: u64,
    /// The proof type (risc0, sp1).
    pub proof_type: String,
    /// The blob proof type.
    pub blob_proof_type: Option<String>,
    /// The prover address.
    pub prover: Option<String>,
    /// The graffiti.
    pub graffiti: Option<String>,
}

impl Default for ProofRequest {
    fn default() -> Self {
        Self {
            l1_chain_id: 1,
            l2_chain_id: 167000,
            batch_id: 0,
            proof_type: "risc0".to_string(),
            blob_proof_type: None,
            prover: None,
            graffiti: None,
        }
    }
}

/// Proof context containing chain specs and request parameters.
#[derive(Debug, Clone)]
pub struct ProofContext {
    pub l1_chain_spec: Arc<RethChainSpec>,
    pub l2_chain_spec: Arc<TaikoChainSpec>,
    pub request: ProofRequest,
    pub config: ProverConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_request_default() {
        let req = ProofRequest::default();
        assert_eq!(req.l1_chain_id, 1);
        assert_eq!(req.l2_chain_id, 167000);
        assert_eq!(req.batch_id, 0);
        assert_eq!(req.proof_type, "risc0");
        assert!(req.blob_proof_type.is_none());
        assert!(req.prover.is_none());
        assert!(req.graffiti.is_none());
    }

    #[test]
    fn test_proof_request_serialization() {
        let req = ProofRequest {
            l1_chain_id: 1,
            l2_chain_id: 167000,
            batch_id: 123,
            proof_type: "sp1".to_string(),
            blob_proof_type: Some("kzg".to_string()),
            prover: Some("0x1234".to_string()),
            graffiti: Some("test".to_string()),
        };

        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ProofRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(req.batch_id, deserialized.batch_id);
        assert_eq!(req.proof_type, deserialized.proof_type);
        assert_eq!(req.blob_proof_type, deserialized.blob_proof_type);
    }
}
