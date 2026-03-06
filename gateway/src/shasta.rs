use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default)]
pub struct ShastaRouteDefaults {
    pub l1_network: String,
    pub network: String,
    pub proof_type: String,
    pub prover: String,
    pub aggregate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShastaRouteKey {
    pub l1_network: String,
    pub network: String,
    pub proof_type: String,
    pub prover: String,
    pub aggregate: bool,
    pub proposal_id: Vec<u64>,
    pub l1_inclusion_block_number: Vec<u64>,
}

#[derive(Debug, Deserialize)]
struct RawShastaRequest {
    l1_network: Option<String>,
    network: Option<String>,
    proof_type: Option<serde_json::Value>,
    prover: Option<serde_json::Value>,
    aggregate: Option<bool>,
    proposals: Vec<RawShastaProposal>,
}

#[derive(Debug, Deserialize)]
struct RawShastaProposal {
    proposal_id: u64,
    l1_inclusion_block_number: u64,
}

pub fn route_key_from_body(body: &[u8]) -> Result<ShastaRouteKey> {
    route_key_from_body_with_defaults(body, &ShastaRouteDefaults::default())
}

pub fn route_key_from_body_with_defaults(
    body: &[u8],
    defaults: &ShastaRouteDefaults,
) -> Result<ShastaRouteKey> {
    let request: RawShastaRequest =
        serde_json::from_slice(body).context("failed to parse shasta request body")?;

    if request.proposals.is_empty() {
        return Err(anyhow!("shasta request must contain at least one proposal"));
    }

    Ok(ShastaRouteKey {
        l1_network: request.l1_network.unwrap_or_else(|| defaults.l1_network.clone()),
        network: request.network.unwrap_or_else(|| defaults.network.clone()),
        proof_type: request
            .proof_type
            .map(value_to_key_string)
            .unwrap_or_else(|| defaults.proof_type.clone()),
        prover: request
            .prover
            .map(value_to_key_string)
            .unwrap_or_else(|| defaults.prover.clone()),
        aggregate: request.aggregate.unwrap_or(defaults.aggregate),
        proposal_id: request.proposals.iter().map(|p| p.proposal_id).collect(),
        l1_inclusion_block_number: request
            .proposals
            .iter()
            .map(|p| p.l1_inclusion_block_number)
            .collect(),
    })
}

pub fn backend_index(route_key: &ShastaRouteKey, backend_replicas: usize) -> usize {
    assert!(backend_replicas > 0, "backend_replicas must be greater than zero");

    let mut hasher = Sha256::new();
    hasher.update(
        serde_json::to_vec(route_key).expect("route key serialization should always succeed"),
    );
    let digest = hasher.finalize();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest[..8]);
    (u64::from_be_bytes(prefix) as usize) % backend_replicas
}

fn value_to_key_string(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value,
        other => other.to_string(),
    }
}
