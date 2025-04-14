#![cfg(feature = "enable")]

use crate::{
    methods::risc0_aggregation::RISC0_AGGREGATION_ELF, methods::risc0_batch::RISC0_BATCH_ELF,
    methods::risc0_guest::RISC0_GUEST_ELF,
};
use alloy_primitives::{hex::ToHexExt, B256};
use bonsai::{cancel_proof, maybe_prove};
use log::{info, warn};
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ZkAggregationGuestInput,
    },
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use risc0_zkvm::{
    compute_image_id, default_prover,
    serde::to_vec,
    sha::{Digest, Digestible},
    ExecutorEnv, ProverOpts, Receipt,
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::fmt::Debug;
use tracing::debug;

pub mod bonsai;
pub mod methods;
pub mod snarks;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Risc0Param {
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
        }
    }
}

pub struct Risc0Prover;

impl Prover for Risc0Prover {
    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut id_store = id_store;
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();
        let proof_key = (
            input.chain_spec.chain_id,
            input.block.header.number,
            output.hash,
            ProofType::Risc0 as u8,
        );

        debug!("elf code length: {}", RISC0_GUEST_ELF.len());
        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");

        let (uuid, receipt) = maybe_prove::<GuestInput, B256>(
            &config,
            encoded_input,
            RISC0_GUEST_ELF,
            &output.hash,
            (Vec::<Receipt>::new(), Vec::new()),
            proof_key,
            &mut id_store,
        )
        .await?;

        let proof_gen_result = if config.snark && config.bonsai {
            bonsai::bonsai_stark_to_snark(uuid, receipt, output.hash, RISC0_GUEST_ELF)
                .await
                .map(|r0_response| r0_response.into())
                .map_err(|e| ProverError::GuestError(e.to_string()))
        } else {
            if !config.snark {
                warn!("proof is not in snark mode, please check.");
            }
            Ok(Risc0Response {
                proof: receipt.journal.encode_hex_with_prefix(),
                receipt: serde_json::to_string(&receipt).unwrap(),
                uuid,
                input: output.hash,
            }
            .into())
        };

        proof_gen_result
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();
        assert!(
            config.snark && config.bonsai,
            "Aggregation must be in bonsai snark mode"
        );

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

        let opts = ProverOpts::groth16();
        let receipt = default_prover()
            .prove_with_opts(env, RISC0_AGGREGATION_ELF, &opts)
            .unwrap()
            .receipt;

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
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut id_store = id_store;
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();
        let proof_key = (
            input.taiko.chain_spec.chain_id,
            input.taiko.batch_id,
            output.hash,
            ProofType::Risc0 as u8,
        );

        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");

        let (uuid, receipt) = maybe_prove::<GuestBatchInput, B256>(
            &config,
            encoded_input,
            RISC0_BATCH_ELF,
            &output.hash,
            (Vec::<Receipt>::new(), Vec::new()),
            proof_key,
            &mut id_store,
        )
        .await?;

        let proof_gen_result = if config.snark && config.bonsai {
            bonsai::bonsai_stark_to_snark(uuid, receipt, output.hash, RISC0_BATCH_ELF)
                .await
                .map(|r0_response| r0_response.into())
                .map_err(|e| ProverError::GuestError(e.to_string()))
        } else {
            if !config.snark {
                warn!("proof is not in snark mode, please check.");
            }
            Ok(Risc0Response {
                proof: receipt.journal.encode_hex_with_prefix(),
                receipt: serde_json::to_string(&receipt).unwrap(),
                uuid,
                input: output.hash,
            }
            .into())
        };

        proof_gen_result
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use methods::risc0_guest::RISC0_GUEST_ID;
    use methods::test_risc0_guest::{TEST_RISC0_GUEST_ELF, TEST_RISC0_GUEST_ID};
    use risc0_zkvm::{default_prover, ExecutorEnv};

    #[test]
    fn run_unittest_elf() {
        std::env::set_var("RISC0_PROVER", "local");
        let env = ExecutorEnv::builder().build().unwrap();
        let prover = default_prover();
        let receipt = prover.prove(env, TEST_RISC0_GUEST_ELF).unwrap();
        receipt.receipt.verify(TEST_RISC0_GUEST_ID).unwrap();
    }

    #[ignore = "only to print image id for docker image build"]
    #[test]
    fn test_show_risc0_image_id() {
        let image_id = RISC0_GUEST_ID
            .map(|limp| hex::encode(limp.to_le_bytes()))
            .concat();
        println!("RISC0 IMAGE_ID: {}", image_id);
    }
}
