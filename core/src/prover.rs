use raiko_lib::{
    consts::VerifierType,
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
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

        ProtocolInstance::new(&input, &header, VerifierType::None)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;

        to_proof(Ok(NativeResponse {
            output: output.clone(),
        }))
    }
}
