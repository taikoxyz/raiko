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

        let pi = ProtocolInstance::new(&input, &output.header, VerifierType::None)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        if pi.instance_hash() != output.hash {
            return Err(ProverError::GuestError(
                "Protocol Instance hash not matched".to_string(),
            ));
        }

        to_proof(Ok(NativeResponse {
            output: output.clone(),
        }))
    }
}
