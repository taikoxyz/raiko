use core::fmt::Debug;
use std::str::FromStr;

use alloy_primitives::{Address, B256};
use anyhow::bail;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub enum ProofType {
    Native,
    Sp1,
    Sgx,
    Risc0,
}

impl FromStr for ProofType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "native" => Ok(ProofType::Native),
            "sp1" => Ok(ProofType::Sp1),
            "sgx" => Ok(ProofType::Sgx),
            "risc0" => Ok(ProofType::Risc0),
            _ => bail!("Unknown prover type: {}", s),
        }
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofRequest {
    /// the block number
    pub block_number: u64,
    /// node for get block by number
    pub rpc: String,
    /// l1 node for signal root verify and get txlist info from proposed transaction.
    pub l1_rpc: String,
    /// beacon node for data blobs
    pub beacon_rpc: String,
    /// network selection
    pub network: String,
    /// l1 network selection
    pub l1_network: String,
    // graffiti
    pub graffiti: B256,
    /// the protocol instance data
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponseError {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub error: JsonRpcError,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse<T> {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<T>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest<T: Serialize> {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: T,
}
