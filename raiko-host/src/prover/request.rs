use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use zeth_primitives::{taiko::ProtocolInstance, TxHash};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum ProofRequest {
    Sgx(SgxRequest),
    PseZk(PseZkRequest),
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
pub struct SgxRequest {
    /// the l1 rpc url
    pub l1_rpc: String,
    /// proposer transaction hash
    #[serde_as(as = "DisplayFromStr")]
    pub l1_propose_block_hash: TxHash,
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
