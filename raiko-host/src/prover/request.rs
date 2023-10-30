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
    pub l2_block: u64,
    /// the l2 rpc url
    pub l2_rpc: String,
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
    /// the instance signature from sgx execution
    pub instance_signature: String,
    /// the signature public key
    pub public_key: String,
    /// SGX
    pub proof: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PseZkResponse {}
