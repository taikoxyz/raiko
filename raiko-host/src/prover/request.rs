use core::fmt::Debug;

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProofType {
    Powdr,
    Sgx,
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofRequest<P> {
    /// the l2 block number
    pub block_number: u64,
    /// l2 node for get block by number
    pub l2_rpc: String,
    /// l1 node for signal root verify and get txlist info from proposed transaction.
    pub l1_rpc: String,
    /// beacon node for data blobs
    pub beacon_rpc: String,
    /// l2 contracts selection
    pub chain: String,
    // graffiti
    pub graffiti: B256,
    /// the protocol instance data
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,
    // Generic proof parameters which has to match with the type
    pub proof_param: P,
}
