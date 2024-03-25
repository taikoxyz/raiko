//! Generate different proofs for the taiko protocol.
use zeth_lib::input::{GuestOutput};

use crate::prover::{
    context::Context,
    request::{ProofRequest, SgxResponse},
};

#[allow(dead_code)]
pub mod cache;

// TODO: driver trait

#[cfg(feature = "powdr")]
pub mod powdr;
#[cfg(not(feature = "powdr"))]
pub mod powdr {

    pub async fn execute_powdr() -> Result<(), String> {
        Err("Feature not powdr is enabled".to_string())
    }
}


#[cfg(feature = "sgx")]
pub mod sgx;
#[cfg(not(feature = "sgx"))]
pub mod sgx {
    use super::*;
    pub async fn execute_sgx(_ctx: &mut Context, _req: &ProofRequest) -> Result<SgxResponse, String> {
        Err("Feature not sgx is enabled".to_string())
    }
}
