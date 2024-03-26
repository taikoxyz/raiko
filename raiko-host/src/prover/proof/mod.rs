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
