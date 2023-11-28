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
    /// l2 node for get block by number
    pub l2_rpc: String,
    /// l1 node for signal root verify and get txlist from proposed transaction.
    pub l1_rpc: String,
    /// the protocol instance data
    pub protocol_instance: ProtocolInstance,
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
