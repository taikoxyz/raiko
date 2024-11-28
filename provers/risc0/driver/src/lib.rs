#![cfg(feature = "enable")]

#[cfg(feature = "bonsai-auto-scaling")]
use crate::bonsai::auto_scaling::shutdown_bonsai;
use crate::{
    methods::risc0_aggregation::RISC0_AGGREGATION_ELF,
    methods::risc0_guest::{RISC0_GUEST_ELF, RISC0_GUEST_ID},
};
use alloy_primitives::{hex::ToHexExt, B256};
use bonsai::{cancel_proof, maybe_prove};
use log::{info, warn};
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestInput, GuestOutput,
        ZkAggregationGuestInput,
    },
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use risc0_zkvm::{
    compute_image_id, default_prover, serde::to_vec, sha::Digestible, ExecutorEnv, ProverOpts,
    Receipt,
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

const RISC0_PROVER_CODE: u8 = 3;

impl Prover for Risc0Prover {
    async fn run(
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
            RISC0_PROVER_CODE,
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
            bonsai::bonsai_stark_to_snark(uuid, receipt, output.hash)
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

        #[cfg(feature = "bonsai-auto-scaling")]
        if config.bonsai {
            // shutdown bonsai
            shutdown_bonsai()
                .await
                .map_err(|e| ProverError::GuestError(e.to_string()))?;
        }

        proof_gen_result
    }

    async fn aggregate(
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
        let input = ZkAggregationGuestInput {
            image_id: RISC0_GUEST_ID,
            block_inputs,
        };
        info!("Start aggregate proofs");
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
            "Generate aggregatino receipt journal: {:?}",
            receipt.journal
        );
        let block_proof_image_id = compute_image_id(RISC0_GUEST_ELF).unwrap();
        let aggregation_image_id = compute_image_id(RISC0_AGGREGATION_ELF).unwrap();
        let proof_data = snarks::verify_aggregation_groth16_proof(
            block_proof_image_id,
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

        #[cfg(feature = "bonsai-auto-scaling")]
        if config.bonsai {
            // shutdown bonsai
            shutdown_bonsai()
                .await
                .map_err(|e| ProverError::GuestError(e.to_string()))?;
        }

        proof_gen_result
    }

    async fn cancel(key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
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
