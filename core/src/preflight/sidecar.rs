use raiko_lib::input::{GuestInput, GuestOutput};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// for raiko use
/// to support use case like raiko reads guest input from remote raiko
/// 


#[derive(Debug, Serialize, ToSchema, Deserialize)]
/// The response body of a proof request.
pub struct ProofResponse {
    #[schema(value_type = Option<GuestInput>)]
    /// The input of the prover.
    pub input: Option<GuestInput>,
    #[schema(value_type = Option<GuestOutputDoc>)]
    /// The output of the prover.
    pub output: Option<GuestOutput>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Status {
    Ok { data: ProofResponse },
    Error { error: String, message: String },
}
