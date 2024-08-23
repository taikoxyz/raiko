use std::path::Path;

use raiko_lib::{
    consts::VerifierType,
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{de::Error, Deserialize, Serialize};
use serde_with::serde_as;
use tracing::trace;

pub struct NativeProver;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeParam {
    pub json_guest_input: Option<String>,
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
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param =
            config
                .get("native")
                .map(NativeParam::deserialize)
                .ok_or(ProverError::Param(serde_json::Error::custom(
                    "native param not provided",
                )))??;

        if let Some(path) = param.json_guest_input {
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

        Ok(Proof {
            ..Default::default()
        })
    }

    async fn cancel(_proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    async fn aggregate(
        input: raiko_lib::input::AggregationGuestInput,
        output: &raiko_lib::input::AggregationGuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        Ok(Proof {
            ..Default::default()
        })
    }
}
