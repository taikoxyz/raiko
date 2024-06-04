use std::{fmt::Debug, fs, path::Path};

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
        
        let param = config.get("native")
            .map(|v| NativeParam::deserialize(v))
            .ok_or( ProverError::Param(serde_json::Error::custom("native param not provided")))??;

        if let Some(path) = param.write_guest_input_path {
            let path = Path::new(&path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?; 
            }
            let json = serde_json::to_string(&input)?;
            fs::write(path, json)?; 
        }

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

