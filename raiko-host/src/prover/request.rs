use serde::{Deserialize, Serialize};
use zeth_primitives::taiko::ProtocolInstance;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum ProofRequest {
    Sgx(SgxRequest),
    PseZk(PseZkRequest),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxRequest {
    /// the l2 block number
    pub block: u64,
    /// the l2 rpc url
    pub rpc: String,
    /// the protocol instance data
    pub protocol_instance: ProtocolInstance,
    /// if run in sgx
    pub no_sgx: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PseZkRequest {}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProofResponse {
    Sgx(SgxResponse),
    PseZk(PseZkResponse),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PseZkResponse {}
