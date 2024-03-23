use std::{env, path::PathBuf};

use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};
use zeth_lib::input::{GuestInput, GuestOutput};

use crate::prover::{context::Context, request::ProofRequest};

pub async fn execute_sp1(
    input: GuestInput,
    output: GuestOutput,
) -> Result<sp1_guest::Sp1Response, String> {
    sp1_guest::execute(input).await
}
