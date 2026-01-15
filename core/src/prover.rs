use std::path::Path;

use raiko_lib::{
    input::{
        GuestBatchInput, GuestBatchOutput, GuestInput, GuestOutput, RawProof,
        ShastaRawAggregationGuestInput,
    },
    proof_type::ProofType,
    protocol_instance::{shasta_pcd_aggregation_hash, ProtocolInstance},
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use reth_primitives::Address;
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
        &self,
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

        let pi = ProtocolInstance::new(&input, &output.header, ProofType::Native)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        if pi.instance_hash() != output.hash {
            return Err(ProverError::GuestError(
                "Protocol Instance hash not matched".to_string(),
            ));
        }

        Ok(Proof {
            input: None,
            proof: None,
            quote: None,
            uuid: None,
            kzg_proof: None,
            extra_data: None,
        })
    }

    async fn batch_run(
        &self,
        batch_input: GuestBatchInput,
        batch_output: &GuestBatchOutput,
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
            let json = serde_json::to_string(&batch_input)?;
            std::fs::write(path, json)?;
        }

        trace!("Running the native prover for batch input: {batch_input:?}");

        let pi = ProtocolInstance::new_batch(
            &batch_input,
            batch_output.blocks.clone(),
            ProofType::Native,
        )
        .map_err(|e| ProverError::GuestError(e.to_string()))?;
        if pi.instance_hash() != batch_output.hash {
            return Err(ProverError::GuestError(
                "Protocol Instance hash not matched".to_string(),
            ));
        }

        Ok(Proof {
            input: Some(batch_output.hash),
            proof: None,
            quote: None,
            uuid: None,
            kzg_proof: None,
            extra_data: None,
        })
    }

    async fn cancel(&self, _proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    async fn aggregate(
        &self,
        _input: raiko_lib::input::AggregationGuestInput,
        _output: &raiko_lib::input::AggregationGuestOutput,
        _config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        Ok(Proof {
            ..Default::default()
        })
    }

    async fn shasta_aggregate(
        &self,
        input: raiko_lib::input::ShastaAggregationGuestInput,
        output: &raiko_lib::input::AggregationGuestOutput,
        _config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        tracing::info!("aggregating shasta proposals: input: {input:?} and output: {output:?}");
        let raw_input = ShastaRawAggregationGuestInput {
            proofs: input
                .proofs
                .iter()
                .map(|proof| RawProof {
                    input: proof.input.clone().unwrap(),
                    proof: Default::default(),
                })
                .collect(),
            proof_carry_data_vec: input
                .proofs
                .iter()
                .map(|proof| proof.extra_data.clone().unwrap())
                .collect(),
        };

        let aggregated_proving_hash =
            shasta_pcd_aggregation_hash(&raw_input.proof_carry_data_vec, Address::ZERO)
                .ok_or_else(|| {
                    ProverError::GuestError(
                        "invalid shasta proof carry data for aggregation".to_string(),
                    )
                })?;
        tracing::info!("aggregated proving hash: {aggregated_proving_hash:?}");

        Ok(Proof {
            ..Default::default()
        })
    }

    fn proof_type(&self) -> ProofType {
        ProofType::Native
    }
}

#[ignore = "Only used to test serialized data"]
#[tokio::test(flavor = "multi_thread")]
async fn test_native_prover() {
    use serde_json::json;

    // Get the current working directory
    let current_dir = std::env::current_dir().expect("Failed to get current directory");

    // Adjust as needed based on your tests
    let file_name = "ethereum-20612846.json";
    let path = current_dir.join("../data").join(file_name);

    // Check if the path exists
    if !path.exists() {
        panic!("File does not exist: {}", path.display());
    }
    let json = std::fs::read_to_string(path).unwrap();

    // Deserialize the input.
    let input: GuestInput = serde_json::from_str(&json).unwrap();
    let output = GuestOutput {
        header: reth_primitives::Header::default(),
        hash: reth_primitives::B256::default(),
    };

    let param = json!({
        "native": {
            "json_guest_input": null
        }
    });
    let prover = NativeProver;
    prover
        .run(input, &output, &param, None)
        .await
        .expect_err("Default output should not match input.");
}
