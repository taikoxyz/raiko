use alloy_primitives::B256;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::{assemble_protocol_instance, ProtocolInstance},
    prover::{to_proof, Proof, Prover, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use tracing::trace;

pub struct NativeProver;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeResponse {
    pub output: GuestOutput,
}

impl Prover for NativeProver {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        _request: &serde_json::Value,
    ) -> ProverResult<Proof> {
        trace!("Running the native prover for input {input:?}");

        let GuestOutput::Success { header, .. } = output.clone() else {
            return Err(ProverError::GuestError("Unexpected output".to_owned()));
        };

        assemble_protocol_instance(&input, &header)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;

        to_proof(Ok(NativeResponse {
            output: output.clone(),
        }))
    }

    fn instance_hash(_pi: ProtocolInstance) -> B256 {
        B256::default()
    }
}
