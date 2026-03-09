#![cfg(feature = "enable")]

use crate::{
    boundless::BoundlessProver, methods::risc0_aggregation::RISC0_AGGREGATION_ELF,
    methods::risc0_batch::RISC0_BATCH_ELF,
    methods::risc0_shasta_aggregation::RISC0_SHASTA_AGGREGATION_ELF,
};
use alloy_primitives::B256;
use bonsai::cancel_proof;
use log::info;
use once_cell::sync::Lazy;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput, ShastaRisc0AggregationGuestInput,
        ZkAggregationGuestInput,
    },
    libhash::hash_shasta_subproof_input,
    proof_type::ProofType,
    protocol_instance::validate_shasta_proof_carry_data_vec,
    prover::{
        IdStore, IdWrite, Proof, ProofCarryData, ProofKey, Prover, ProverConfig, ProverError,
        ProverResult,
    },
};
use risc0_zkvm::{
    compute_image_id, get_prover_server,
    serde::to_vec,
    sha::{Digest, Digestible},
    ExecutorEnv, ExecutorImpl, ProverOpts, Receipt, VerifierContext,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::serde_as;
use std::fmt::Debug;

pub mod bonsai;
pub mod boundless;
pub mod methods;
pub mod snarks;

static SHASTA_AGGREGATION_PROGRAM_HASH: Lazy<String> = Lazy::new(|| {
    hex::encode(
        Digest::from(methods::risc0_shasta_aggregation::RISC0_SHASTA_AGGREGATION_ID).as_bytes(),
    )
});

static AGGREGATION_PROGRAM_HASH: Lazy<String> = Lazy::new(|| {
    hex::encode(Digest::from(methods::risc0_aggregation::RISC0_AGGREGATION_ID).as_bytes())
});

static BLOCK_PROGRAM_HASH: Lazy<String> =
    Lazy::new(|| hex::encode(Digest::from(methods::risc0_batch::RISC0_BATCH_ID).as_bytes()));

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Risc0Param {
    pub boundless: bool,
    pub bonsai: bool,
    pub snark: bool,
    pub profile: bool,
    pub execution_po2: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Risc0Response {
    pub proof: String,
    pub receipt: String,
    pub uuid: String,
    pub input: B256,
}

impl From<Risc0Response> for Proof {
    fn from(value: Risc0Response) -> Self {
        Self {
            proof: Some(value.proof),
            quote: Some(value.receipt),
            input: Some(value.input),
            uuid: Some(value.uuid),
            kzg_proof: None,
            extra_data: None,
        }
    }
}

pub struct Risc0Prover;

impl Prover for Risc0Prover {
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
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let boundless_cfg = config;
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();

        if config.boundless {
            // Delegate to boundless driver (agent-managed) when enabled.
            return BoundlessProver::new()
                .aggregate(input, _output, boundless_cfg, None)
                .await
                .map_err(|e| ProverError::GuestError(e.to_string()));
        }

        assert!(config.snark, "Aggregation must be in snark mode");

        // Extract the block proof receipts
        let assumptions: Vec<Receipt> = input
            .proofs
            .iter()
            .map(|proof| {
                let receipt: Receipt = serde_json::from_str(&proof.quote.clone().unwrap())
                    .expect("Failed to deserialize");
                receipt
            })
            .collect::<Vec<_>>();
        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .map(|proof| proof.input.unwrap())
            .collect::<Vec<_>>();

        let input_proof_hex_str = input.proofs[0].proof.as_ref().unwrap();
        let input_proof_bytes = hex::decode(&input_proof_hex_str[2..]).unwrap();
        let input_image_id_bytes: [u8; 32] = input_proof_bytes[32..64].try_into().unwrap();
        let input_proof_image_id = Digest::from(input_image_id_bytes);
        let input = ZkAggregationGuestInput {
            image_id: input_proof_image_id.as_words().try_into().unwrap(),
            block_inputs,
        };

        // add_assumption makes the receipt to be verified available to the prover.
        let env = {
            let mut env = ExecutorEnv::builder();
            for assumption in assumptions {
                env.add_assumption(assumption);
            }
            env.write(&input).unwrap().build().unwrap()
        };

        info!("Running RISC0 aggregation proof locally (Groth16)...");
        let receipt = {
            let mut exec = ExecutorImpl::from_elf(env, RISC0_AGGREGATION_ELF)
                .map_err(|e| ProverError::GuestError(format!("Executor init failed: {e}")))?;
            let session = exec
                .run()
                .map_err(|e| ProverError::GuestError(format!("Execution failed: {e}")))?;
            let opts = ProverOpts::groth16();
            let prover = get_prover_server(&opts)
                .map_err(|e| ProverError::GuestError(format!("Prover init failed: {e}")))?;
            prover
                .prove_session(&VerifierContext::default(), &session)
                .map_err(|e| {
                    tracing::error!("Failed to generate RISC0 aggregation proof: {:?}", e);
                    ProverError::GuestError(format!(
                        "RISC0 aggregation proof generation failed: {}",
                        e
                    ))
                })?
                .receipt
        };

        info!(
            "Generate aggregation receipt journal: {:?}",
            alloy_primitives::hex::encode_prefixed(receipt.journal.bytes.clone())
        );
        let aggregation_image_id = compute_image_id(RISC0_AGGREGATION_ELF).unwrap();
        let proof_data = snarks::verify_aggregation_groth16_proof(
            input_proof_image_id,
            aggregation_image_id,
            receipt.clone(),
        )
        .await
        .map_err(|err| format!("Failed to verify SNARK: {err:?}"))?;
        let snark_proof = alloy_primitives::hex::encode_prefixed(proof_data);

        info!("Aggregation proof: {snark_proof:?}");
        let proof_gen_result = Ok(Risc0Response {
            proof: snark_proof,
            receipt: serde_json::to_string(&receipt).unwrap(),
            uuid: "".to_owned(),
            input: B256::from_slice(receipt.journal.digest().as_bytes()),
        }
        .into());

        proof_gen_result
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
        cancel_proof(uuid)
            .await
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        id_store.remove_id(key).await
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let boundless_cfg = config;
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();

        if config.boundless {
            // Delegate to boundless driver (agent-managed) when enabled.
            return BoundlessProver::new()
                .batch_run(input, output, boundless_cfg, None)
                .await
                .map_err(|e| ProverError::GuestError(e.to_string()));
        }

        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");

        info!(
            "Running RISC0 batch proof locally (execution_po2={})...",
            config.execution_po2
        );

        // Use Succinct (not Groth16) — batch proofs only need to be valid assumptions
        // for the aggregation step. Groth16 wrapping is expensive and only needed once
        // at aggregation time for on-chain verification.
        let opts = ProverOpts::succinct();

        // Prove locally — uses CUDA when the `cuda` feature is enabled.
        let receipt = bonsai::prove_locally(
            config.execution_po2,
            encoded_input,
            RISC0_BATCH_ELF,
            Vec::<Receipt>::new(),
            config.profile,
            &opts,
        )?;

        // Verify output
        let output_guest: B256 = receipt
            .journal
            .decode()
            .map_err(|e| ProverError::GuestError(format!("Failed to decode journal: {e}")))?;
        if output.hash != output_guest {
            return Err(ProverError::GuestError(format!(
                "Output mismatch! Prover: {output_guest:?}, expected: {:?}",
                output.hash
            )));
        }
        info!("Local batch proof output verified.");

        // Build the proof response. aggregate() reads:
        //   .quote  → serialized Receipt (used as assumption)
        //   .input  → B256 hash
        //   .proof  → hex string with image_id at bytes 32..64
        // No on-chain verification needed — only the final aggregation proof goes on-chain.
        let image_id = compute_image_id(RISC0_BATCH_ELF)
            .map_err(|e| ProverError::GuestError(format!("Failed to compute image id: {e}")))?;
        let mut proof_bytes = vec![0u8; 64];
        proof_bytes[32..64].copy_from_slice(image_id.as_bytes());
        let proof_hex = format!("0x{}", hex::encode(proof_bytes));

        Ok(Risc0Response {
            proof: proof_hex,
            receipt: serde_json::to_string(&receipt).map_err(|e| {
                ProverError::GuestError(format!("Failed to serialize receipt: {e}"))
            })?,
            uuid: String::new(),
            input: output.hash,
        }
        .into())
    }

    async fn get_guest_data() -> ProverResult<serde_json::Value> {
        Ok(json!({
            "risc0": {
                "aggregation_program_hash": AGGREGATION_PROGRAM_HASH.to_string(),
                "block_program_hash": BLOCK_PROGRAM_HASH.to_string(),
                "shasta_aggregation_program_hash": SHASTA_AGGREGATION_PROGRAM_HASH.to_string(),
            }
        }))
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let boundless_cfg = config;
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();

        if config.boundless {
            // Delegate to boundless driver (agent-managed) when enabled.
            return BoundlessProver::new()
                .shasta_aggregate(input, _output, boundless_cfg, None)
                .await
                .map_err(|e| ProverError::GuestError(e.to_string()));
        }

        assert!(config.snark, "Shasta aggregation must be in snark mode");

        let assumptions: Vec<Receipt> = input
            .proofs
            .iter()
            .map(|proof| {
                let receipt: Receipt = serde_json::from_str(&proof.quote.clone().unwrap())
                    .expect("Failed to deserialize");
                receipt
            })
            .collect::<Vec<_>>();
        let proof_carry_data_vec: Vec<ProofCarryData> = input
            .proofs
            .iter()
            .map(|proof| {
                proof.extra_data.clone().ok_or_else(|| {
                    ProverError::GuestError("missing shasta proof carry data".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let block_inputs = build_shasta_block_inputs(&input.proofs, &proof_carry_data_vec)?;

        let input_proof_hex_str = input.proofs[0].proof.as_ref().unwrap();
        let input_proof_bytes = hex::decode(&input_proof_hex_str[2..]).unwrap();
        let input_image_id_bytes: [u8; 32] = input_proof_bytes[32..64].try_into().unwrap();
        let input_proof_image_id = Digest::from(input_image_id_bytes);

        let shasta_input = ShastaRisc0AggregationGuestInput {
            image_id: input_proof_image_id.as_words().try_into().unwrap(),
            block_inputs: block_inputs.clone(),
            proof_carry_data_vec,
        };

        let env = {
            let mut env = ExecutorEnv::builder();
            for assumption in assumptions {
                env.add_assumption(assumption);
            }
            env.write(&shasta_input).unwrap().build().unwrap()
        };

        info!("Running RISC0 shasta aggregation proof locally (Groth16)...");
        let receipt = {
            let mut exec = ExecutorImpl::from_elf(env, RISC0_SHASTA_AGGREGATION_ELF)
                .map_err(|e| ProverError::GuestError(format!("Executor init failed: {e}")))?;
            let session = exec
                .run()
                .map_err(|e| ProverError::GuestError(format!("Execution failed: {e}")))?;
            let opts = ProverOpts::groth16();
            let prover = get_prover_server(&opts)
                .map_err(|e| ProverError::GuestError(format!("Prover init failed: {e}")))?;
            prover
                .prove_session(&VerifierContext::default(), &session)
                .map_err(|e| {
                    tracing::error!("Failed to generate RISC0 shasta aggregation proof: {:?}", e);
                    ProverError::GuestError(format!(
                        "RISC0 shasta aggregation proof generation failed: {}",
                        e
                    ))
                })?
                .receipt
        };

        info!(
            "Generate shasta aggregation receipt journal: {:?}",
            alloy_primitives::hex::encode_prefixed(receipt.journal.bytes.clone())
        );
        let aggregation_image_id = compute_image_id(RISC0_SHASTA_AGGREGATION_ELF).unwrap();
        let proof_data = snarks::verify_aggregation_groth16_proof(
            input_proof_image_id,
            aggregation_image_id,
            receipt.clone(),
        )
        .await
        .map_err(|err| format!("Failed to verify SNARK: {err:?}"))?;
        let snark_proof = alloy_primitives::hex::encode_prefixed(proof_data);

        info!("Shasta aggregation proof: {snark_proof:?}");
        Ok::<_, ProverError>(
            Risc0Response {
                proof: snark_proof,
                receipt: serde_json::to_string(&receipt).unwrap(),
                uuid: "".to_owned(),
                input: B256::from_slice(receipt.journal.digest().as_bytes()),
            }
            .into(),
        )
    }

    fn proof_type(&self) -> ProofType {
        ProofType::Risc0
    }
}

fn build_shasta_block_inputs(
    proofs: &[Proof],
    proof_carry_data_vec: &[ProofCarryData],
) -> ProverResult<Vec<B256>> {
    if proofs.len() != proof_carry_data_vec.len() {
        return Err(ProverError::GuestError(
            "shasta proofs length mismatch with carry data".to_string(),
        ));
    }
    if !validate_shasta_proof_carry_data_vec(proof_carry_data_vec) {
        return Err(ProverError::GuestError(
            "invalid shasta proof carry data".to_string(),
        ));
    }

    let mut block_inputs = Vec::with_capacity(proofs.len());
    for (idx, (proof, carry)) in proofs.iter().zip(proof_carry_data_vec).enumerate() {
        let proof_input = proof
            .input
            .ok_or_else(|| ProverError::GuestError("missing shasta proof public input".into()))?;
        let expected = hash_shasta_subproof_input(carry);
        if proof_input != expected {
            return Err(ProverError::GuestError(format!(
                "shasta proof input mismatch at index {idx}"
            )));
        }
        block_inputs.push(proof_input);
    }

    Ok(block_inputs)
}

#[cfg(test)]
mod test {
    use super::*;
    use methods::risc0_batch::RISC0_BATCH_ID;
    use methods::test_risc0_batch::{TEST_RISC0_BATCH_ELF, TEST_RISC0_BATCH_ID};
    use risc0_zkvm::{default_prover, ExecutorEnv};

    #[test]
    fn run_unittest_elf() {
        std::env::set_var("RISC0_PROVER", "local");
        let env = ExecutorEnv::builder().build().unwrap();
        let prover = default_prover();
        let receipt = prover.prove(env, TEST_RISC0_BATCH_ELF).unwrap();
        receipt.receipt.verify(TEST_RISC0_BATCH_ID).unwrap();
    }

    #[ignore = "only to print image id for docker image build"]
    #[test]
    fn test_show_risc0_image_id() {
        let image_id = RISC0_BATCH_ID
            .map(|limp| hex::encode(limp.to_le_bytes()))
            .concat();
        println!("RISC0 IMAGE_ID: {}", image_id);
    }
}
