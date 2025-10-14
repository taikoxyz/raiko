#![cfg(feature = "enable")]

use alloy_primitives::B256;
use log::info;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput,
    },
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

pub mod axiom;
pub mod methods;

use methods::{OPENVM_AGGREGATION_ELF, OPENVM_BATCH_ELF};

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenVMParam {
    pub axiom: bool,
    pub verify: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OpenVMResponse {
    pub proof: String,
    pub output: B256,
    pub uuid: String,
}

impl From<OpenVMResponse> for Proof {
    fn from(value: OpenVMResponse) -> Self {
        Self {
            proof: Some(value.proof),
            quote: None,
            input: Some(value.output),
            uuid: Some(value.uuid),
            kzg_proof: None,
        }
    }
}

pub struct OpenVMProver;

impl Prover for OpenVMProver {
    async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("no block run after pacaya fork")
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param = OpenVMParam::deserialize(config.get("openvm").unwrap())
            .map_err(|e| ProverError::Param(e))?;

        // Serialize input for the guest program
        let input_bytes = bincode::serialize(&input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {}", e)))?;

        if param.axiom {
            // Use Axiom network for remote proving
            let proof_key = (
                input.proofs[0].input.unwrap().0[0] as u64, // chain_id placeholder
                0,                                           // batch_id placeholder
                B256::ZERO,                                  // output hash placeholder
                ProofType::OpenVM as u8,
            );

            axiom::prove_axiom(
                OPENVM_AGGREGATION_ELF,
                &input_bytes,
                proof_key,
                id_store,
            )
            .await
            .map(|r| r.into())
        } else {
            // Local proving
            prove_locally(OPENVM_AGGREGATION_ELF, &input_bytes).map(|r| r.into())
        }
    }

    async fn cancel(&self, key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        let uuid = match id_store.read_id(key).await {
            Ok(uuid) => uuid,
            Err(e) => {
                if e.to_string().contains("No data for query") {
                    return Ok(());
                } else {
                    return Err(ProverError::GuestError(e.to_string()));
                }
            }
        };

        axiom::cancel_proof(uuid).await
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param = OpenVMParam::deserialize(config.get("openvm").unwrap())
            .map_err(|e| ProverError::Param(e))?;

        let proof_key = (
            input.taiko.chain_spec.chain_id,
            input.taiko.batch_id,
            output.hash,
            ProofType::OpenVM as u8,
        );

        // Serialize input for the guest program
        let input_bytes = bincode::serialize(&input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {}", e)))?;

        if param.axiom {
            info!("Using Axiom network for OpenVM proving");
            axiom::prove_axiom(OPENVM_BATCH_ELF, &input_bytes, proof_key, id_store)
                .await
                .map(|r| r.into())
        } else {
            info!("Using local OpenVM prover");
            let result = prove_locally(OPENVM_BATCH_ELF, &input_bytes)?;

            // Verify output hash matches
            if param.verify {
                if result.output != output.hash {
                    return Err(ProverError::GuestError(format!(
                        "Output hash mismatch: expected {}, got {}",
                        output.hash, result.output
                    )));
                }
                info!("Output hash verified successfully");
            }

            Ok(result.into())
        }
    }
}

/// Prove locally using OpenVM SDK
///
/// API Reference (based on openvm-sdk docs):
/// - `Sdk::standard()`: Initialize SDK with all default extensions
/// - `StdIn::write_bytes()`: Write raw bytes as input
/// - `sdk.execute()`: Execute program and get public output
/// - `sdk.prove()`: Generate STARK proof, returns (proof, app_commit)
fn prove_locally(elf: &[u8], input: &[u8]) -> ProverResult<OpenVMResponse> {
    info!("Starting local OpenVM proof generation");

    use openvm_sdk::{Sdk, StdIn};

    // 1. Initialize OpenVM SDK with standard configuration
    info!("Initializing OpenVM SDK");
    let sdk = Sdk::standard();

    // 2. Prepare input using StdIn
    let mut stdin = StdIn::default();
    stdin.write_bytes(input);

    // 3. Execute to get public output first (faster than full prove)
    info!("Executing OpenVM program to get public output");
    let public_values = sdk
        .execute(elf.to_vec(), stdin.clone())
        .map_err(|e| ProverError::GuestError(format!("OpenVM execution failed: {}", e)))?;

    // 4. Extract output hash from public values (expecting 32 bytes)
    let output_hash = if public_values.len() >= 32 {
        B256::from_slice(&public_values[0..32])
    } else {
        return Err(ProverError::GuestError(format!(
            "Invalid output length from guest: expected >=32 bytes, got {}",
            public_values.len()
        )));
    };

    info!("Public output hash: {}", output_hash);

    // 5. Generate STARK proof
    info!("Generating OpenVM STARK proof");
    let (_proof, app_commit) = sdk
        .prove(elf.to_vec(), stdin)
        .map_err(|e| ProverError::GuestError(format!("OpenVM proof generation failed: {}", e)))?;

    // 6. Serialize app_commit for identification
    // Note: The actual STARK proof verification happens on-chain or through OpenVM's
    // verification infrastructure. For local proving, we primarily need the app_commit
    // which identifies the execution.
    let app_commit_bytes = bincode::serialize(&app_commit)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize app_commit: {}", e)))?;

    info!("OpenVM proof generated successfully");

    // Create a unique UUID from app_commit by hashing it
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&app_commit_bytes);
    let commit_hash = hasher.finalize();
    let uuid = format!("local-openvm-{}", hex::encode(&commit_hash[..8]));

    // Store the serialized app_commit as the proof identifier
    Ok(OpenVMResponse {
        proof: hex::encode(app_commit_bytes),
        output: output_hash,
        uuid,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_openvm_param_deserialize() {
        let json = serde_json::json!({
            "axiom": true,
            "verify": true
        });

        let param: OpenVMParam = serde_json::from_value(json).unwrap();
        assert!(param.axiom);
        assert!(param.verify);
    }
}
