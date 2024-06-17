use std::path::Path;

use raiko_lib::{
    consts::VerifierType,
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{de::Error, Deserialize, Serialize};
use serde_with::serde_as;
use tracing::trace;

pub struct NativeProver;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeParam {
    pub write_guest_input_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeResponse {
    pub output: GuestOutput,
}

impl Prover for NativeProver {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let param =
            config
                .get("native")
                .map(NativeParam::deserialize)
                .ok_or(ProverError::Param(serde_json::Error::custom(
                    "native param not provided",
                )))??;

        if let Some(path) = param.write_guest_input_path {
            let path = Path::new(&path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string(&input)?;
            std::fs::write(path, json)?;
        }

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
