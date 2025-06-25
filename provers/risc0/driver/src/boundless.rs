use crate::{
    methods::{
        boundless_aggregation::BOUNDLESS_AGGREGATION_ELF, boundless_batch::BOUNDLESS_BATCH_ELF,
    },
    snarks::verify_boundless_groth16_snark_impl,
    Risc0Response,
};
use alloy_primitives::B256;
use alloy_primitives_v8::address;
use alloy_signer_local_v12::PrivateKeySigner;
use alloy_sol_types::SolValue;
use boundless_market::{
    client::ClientBuilder,
    contracts::{Input, Offer, Predicate, ProofRequestBuilder, Requirements},
    input::InputBuilder,
    storage::storage_provider_from_env,
};
use raiko_lib::primitives::utils::parse_ether;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput,
    },
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use reqwest::Url;
use risc0_ethereum_contracts_boundless::receipt::Receipt as ContractReceipt;
use risc0_zkvm::{
    compute_image_id, default_executor, serde::to_vec, sha::Digestible, Digest,
    Receipt as ZkvmReceipt,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Risc0AgengAggGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<ZkvmReceipt>,
}

// share with agent, need a unified place for this
// now just copy from agent
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Risc0AgentResponse {
    pub seal: Vec<u8>,
    pub journal: Vec<u8>,
    pub receipt: Option<String>,
}

pub struct Risc0BoundlessProver;

impl Risc0BoundlessProver {
    pub async fn prepare_batch_run(
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        _config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> Result<(), ProverError> {
        // Save the input to a bincode file for debugging or inspection.
        let file_name = format!("/tmp/batch-{}-input.bin", input.taiko.batch_id);
        if let Ok(encoded) = bincode::serialize(&input) {
            if let Err(e) = std::fs::write(&file_name, &encoded) {
                tracing::warn!("Failed to write input to {}: {}", file_name, e);
                Err(ProverError::GuestError(format!(
                    "Failed to write input to {}: {}",
                    file_name, e
                )))
            } else {
                tracing::info!("Saved input to bincode file: {}", file_name);
                Ok(())
            }
        } else {
            tracing::warn!(
                "Failed to serialize input to bincode for file: {}",
                file_name
            );
            Err(ProverError::GuestError(format!(
                "Failed to serialize input to bincode for file: {}",
                file_name
            )))
        }
    }

    pub async fn prepare_aggregation_run(
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> Result<(), ProverError> {
        let file_name = format!(
            "/tmp/aggregation-input-{}.bin",
            input.proofs[0].input.unwrap_or_default()
        );

        let image_id = compute_image_id(BOUNDLESS_BATCH_ELF).unwrap();
        let agent_input = Risc0AgengAggGuestInput {
            image_id: image_id,
            receipts: input
                .proofs
                .iter()
                .map(|p| {
                    let receipt_json = p.quote.clone().unwrap();
                    let receipt: ZkvmReceipt = serde_json::from_str(&receipt_json).unwrap();
                    receipt
                })
                .collect(),
        };
        if let Ok(encoded) = bincode::serialize(&agent_input) {
            if let Err(e) = std::fs::write(&file_name, &encoded) {
                tracing::warn!("Failed to write input to {}: {}", file_name, e);
                Err(ProverError::GuestError(format!(
                    "Failed to write input to {}: {}",
                    file_name, e
                )))
            } else {
                tracing::info!("Saved input to bincode file: {}", file_name);
                Ok(())
            }
        } else {
            tracing::warn!(
                "Failed to serialize input to bincode for file: {}",
                file_name
            );
            Err(ProverError::GuestError(format!(
                "Failed to serialize input to bincode for file: {}",
                file_name
            )))
        }
    }
}

impl Prover for Risc0BoundlessProver {
    async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("No need for post pacaya");
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Construct the input file name for aggregation, consistent with prepare_aggregation_run
        let task_id = input.proofs[0].input.unwrap_or_default();
        let input_file = format!("/tmp/aggregation-input-{}.bin", task_id);
        let output_file = format!("/tmp/aggregation-proof-{}.bin", task_id);

        // Write the input to file
        let encoded_input = bincode::serialize(&input).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize aggregation input: {e}"))
        })?;
        std::fs::write(&input_file, &encoded_input).map_err(|e| {
            ProverError::GuestError(format!("Failed to write aggregation input file: {e}"))
        })?;

        // Build the command to run the boundless-agent for aggregation
        let cmd = format!(
            "./target/release/boundless-agent --input {} --output {} --proof-type aggregation --verbose",
            input_file, output_file
        );

        // Run the command
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to execute aggregation command: {e}"))
            })?;

        if !output.status.success() {
            return Err(ProverError::GuestError(format!(
                "Aggregation command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        // Read the output proof file
        let agent_proof_bytes = std::fs::read(&output_file).map_err(|e| {
            ProverError::GuestError(format!("Failed to read aggregation proof file: {e}"))
        })?;
        let agent_proof: Risc0AgentResponse =
            bincode::deserialize(&agent_proof_bytes).map_err(|e| {
                ProverError::GuestError(format!("Failed to deserialize output file: {e}"))
            })?;

        let image_id = compute_image_id(BOUNDLESS_AGGREGATION_ELF).unwrap();
        let journal_digest = agent_proof.journal.digest();
        let encoded_proof = verify_boundless_groth16_snark_impl(
            image_id,
            agent_proof.seal.to_vec(),
            journal_digest,
        )
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to verify groth16 snark: {e}")))?;
        let proof: Vec<u8> = (encoded_proof, B256::from_slice(image_id.as_bytes()))
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect();

        Ok(Proof {
            proof: Some(alloy_primitives::hex::encode_prefixed(proof)),
            input: Some(B256::from_slice(agent_proof.journal.as_slice())),
            quote: None,
            uuid: None,
            kzg_proof: None,
        })
    }

    async fn cancel(&self, _key: ProofKey, _id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        todo!()
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Construct the input file name based on the batch id
        let id = input.taiko.batch_id;
        let input_file = format!("/tmp/input-{}.bin", id);
        let output_file = format!("/tmp/batch-proof-{}.bin", id);

        // Build the command
        let cmd = format!(
            "./target/release/boundless-agent --input {} --output {} --proof-type batch --verbose",
            input_file, output_file
        );

        // Run the command
        let cmd_stdout = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .map_err(|e| ProverError::GuestError(format!("Failed to run boundless-agent: {e}")))?;

        if !cmd_stdout.status.success() {
            let stderr = String::from_utf8_lossy(&cmd_stdout.stderr);
            return Err(ProverError::GuestError(format!(
                "boundless-agent failed: {stderr}"
            )));
        }

        // Try to read the output file
        let agent_proof_bytes = std::fs::read(&output_file)
            .map_err(|e| ProverError::GuestError(format!("Failed to read output file: {e}")))?;
        let agent_proof: Risc0AgentResponse =
            bincode::deserialize(&agent_proof_bytes).map_err(|e| {
                ProverError::GuestError(format!("Failed to deserialize output file: {e}"))
            })?;

        let image_id = compute_image_id(BOUNDLESS_BATCH_ELF).unwrap();
        let journal_digest = agent_proof.journal.digest();
        let encoded_proof = verify_boundless_groth16_snark_impl(
            image_id,
            agent_proof.seal.to_vec(),
            journal_digest,
        )
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to verify groth16 snark: {e}")))?;
        let proof_bytes: Vec<u8> = (encoded_proof, B256::from_slice(image_id.as_bytes()))
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect();
        assert_eq!(agent_proof.journal, output.hash.to_vec());
        Ok(Risc0Response {
            proof: alloy_primitives::hex::encode_prefixed(proof_bytes),
            receipt: agent_proof.receipt.unwrap(),
            uuid: "".to_string(), // can be request tx hash
            input: output.hash,
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_logger;
    use raiko_lib::input::GuestBatchOutput;

    #[tokio::test]
    async fn test_batch_run() {
        Risc0BoundlessProver::init_prover().await.unwrap();
    }

    #[tokio::test]
    async fn test_run_prover() {
        // init log
        env_logger::init();

        let input_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/input-1306738.json").unwrap();
        let output_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/output-1306738.json").unwrap();
        let input: GuestBatchInput = serde_json::from_slice(&input_file).unwrap();
        let output: GuestBatchOutput = serde_json::from_slice(&output_file).unwrap();
        let config = ProverConfig::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover
            .batch_run(input, &output, &config, None)
            .await
            .unwrap();
        println!("proof: {:?}", proof);

        // Save the boundless_receipt as JSON to a file for later deserialization.
        // The file name can be based on the request_id or image_id for uniqueness.
        let receipt_json = serde_json::to_string_pretty(&proof).unwrap();
        let file_name = format!("../../../boundless_receipt_test.json");
        if let Err(e) = std::fs::write(&file_name, receipt_json) {
            tracing::warn!(
                "Failed to write boundless_receipt to file {}: {}",
                file_name,
                e
            );
        } else {
            tracing::info!("Saved boundless_receipt to file: {}", file_name);
        }
    }

    #[tokio::test]
    async fn test_transfer_input_output() {
        // init log
        env_logger::init();

        let input_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/input-1306738.json").unwrap();
        let output_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/output-1306738.json").unwrap();
        let input: GuestBatchInput = serde_json::from_slice(&input_file).unwrap();
        let output: GuestBatchOutput = serde_json::from_slice(&output_file).unwrap();

        let input_bytes = bincode::serialize(&input).unwrap();
        let output_bytes = bincode::serialize(&output).unwrap();
        // println!("input_bytes: {:?}", input_bytes);
        // println!("output_bytes: {:?}", output_bytes);

        //save to file
        let input_file_name = format!("../../../input-1306738.bin");
        let output_file_name = format!("../../../output-1306738.bin");
        std::fs::write(&input_file_name, input_bytes).unwrap();
        std::fs::write(&output_file_name, output_bytes).unwrap();
        println!("Saved input to file: {}", input_file_name);
        println!("Saved output to file: {}", output_file_name);

        // deserialize from data & check equality
        let input_bytes = std::fs::read(&input_file_name).unwrap();
        let output_bytes = std::fs::read(&output_file_name).unwrap();
        let input_deserialized: GuestBatchInput =
            bincode::deserialize(&input_bytes).expect("Failed to deserialize input");
        let output_deserialized: GuestBatchOutput =
            bincode::deserialize(&output_bytes).expect("Failed to deserialize output");
    }

    #[tokio::test]
    async fn test_run_prover_with_seal() {
        env_logger::init();

        let seal = alloy_primitives::hex::decode("0x9f39696c021c04f95caa9962aa0022f0eae58f1cd7e13ccf553a152a3d0e91443d0aab4f25a24e93423c51f1ae46e604e20a360cfe2376e7270a10d1f4a9e665adcc91e713155b2e45e05edb00c7f044ab827a425cac6d0c932e3e14aeddf79200a8fe7711ad2207298cf2004c5dffc5956e9b30d6b98e9e2533b1e6944671f35dacf85823bb4fd3e0dd14a0000bc3304338f844b11095d1dbfedf3e90074bf7c666ed531dd4676c51fdf0111529d5c40719d36ba8ba11db8542fff1bca90c24255c515f1b6e32a396bf2bdb40ad165f949f1d46c533266a666e3b6684ddbbbc8c4ce5c1051676d81b1addd377e8b9665912d32347aac64c1a9b38faaab63ceeb1dcc67c").unwrap();
        let image_id = compute_image_id(RISC0_BATCH_ELF).unwrap();
        let journal = alloy_primitives::hex::decode(
            "0x20000000b0284764aae7f327410ae1355bef23dcccd0f9c723724d6638a8edde86091d65",
        )
        .unwrap();
        let journal_digest = journal.digest();
        let encoded_proof =
            verify_boundless_groth16_snark_impl(image_id, seal, journal_digest.into())
                .await
                .unwrap();
        println!("encoded_proof: {:?}", encoded_proof);
    }

    #[test]
    fn test_deserialize_proof() {
        // This test deserializes a proof from a JSON string (as would be returned from Boundless).
        let proof_json = r#"{
             "proof": "0x0000000000000000000000000000000000000000000000000000000000000040a9b03d0dd651aebfd77634799760072e8392c3c91e17d7c3da6785a61aaffdbe00000000000000000000000000000000000000000000000000000000000001049f39696c117e359f6a322d19b2ea8437271cda231c152d70fb553c6ed68e5c90e05c307c2787e39785bdec77c7cd712005367690160274f270397d7eca1e103c5633f7711ea988975445d70d2ce30d4da7648aa55d311b3796ffb35b3648ee7dd848f150002db50185bbc16d3aacf2d5ea19fe9368361b57ebc8590df4f637a91a142a32200efe06906e1e33c0e2caa7e8e9bec6aa0289e7f4ccb771ababe0a7df5e82960633839ddff0e44685ad0b9f137da03fd51cbeccc3d6cd163c83395814ed3d9618aca53e3ec65562300fee630606e22fe2b84c70a63dd60ffc42781f4d49ca08016bbe2581766d96144b1c90eb1eb65cfba92e9b4353c1fb9a6e89b957e3c1bf00000000000000000000000000000000000000000000000000000000",
             "input": "0x6f478ee63e81d8f341716638ebb2c524884af8441de92aed284176210169e942",
             "quote": "{\"inner\":{\"Groth16\":{\"seal\":[17,126,53,159,106,50,45,25,178,234,132,55,39,28,218,35,28,21,45,112,251,85,60,110,214,142,92,144,224,92,48,124,39,135,227,151,133,189,236,119,199,205,113,32,5,54,118,144,22,2,116,242,112,57,125,126,202,30,16,60,86,51,247,113,30,169,136,151,84,69,215,13,44,227,13,77,167,100,138,165,93,49,27,55,150,255,179,91,54,72,238,125,216,72,241,80,0,45,181,1,133,187,193,109,58,172,242,213,234,25,254,147,104,54,27,87,235,200,89,13,244,246,55,169,26,20,42,50,32,14,254,6,144,110,30,51,192,226,202,167,232,233,190,198,170,2,137,231,244,204,183,113,171,171,224,167,223,94,130,150,6,51,131,157,223,240,228,70,133,173,11,159,19,125,160,63,213,28,190,204,195,214,205,22,60,131,57,88,20,237,61,150,24,172,165,62,62,198,85,98,48,15,238,99,6,6,226,47,226,184,76,112,166,61,214,15,252,66,120,31,77,73,202,8,1,107,190,37,129,118,109,150,20,75,28,144,235,30,182,92,251,169,46,155,67,83,193,251,154,110,137,185,87,227,193,191],\"claim\":{\"Value\":{\"pre\":{\"Pruned\":[222146729,3215872470,2033481431,772235415,3385037443,3285653278,2793760730,3204296474]},\"post\":{\"Value\":{\"pc\":0,\"merkle_root\":[0,0,0,0,0,0,0,0]}},\"exit_code\":{\"Halted\":0},\"input\":{\"Value\":null},\"output\":{\"Value\":{\"journal\":{\"Value\":[32,0,0,0,176,40,71,100,170,231,243,39,65,10,225,53,91,239,35,220,204,208,249,199,35,114,77,102,56,168,237,222,134,9,29,101]},\"assumptions\":{\"Pruned\":[0,0,0,0,0,0,0,0]}}}}},\"verifier_parameters\":[1818835359,1620946611,2780288568,2130774364,576647948,727242602,2964052866,2234770906]}},\"journal\":{\"bytes\":[32,0,0,0,176,40,71,100,170,231,243,39,65,10,225,53,91,239,35,220,204,208,249,199,35,114,77,102,56,168,237,222,134,9,29,101]},\"metadata\":{\"verifier_parameters\":[1818835359,1620946611,2780288568,2130774364,576647948,727242602,2964052866,2234770906]}}}",
             "uuid": "",
             "kzg_proof": null
         }"#;

        // The ContractReceipt type is used for Boundless receipts.
        let proof: Proof =
            serde_json::from_str(proof_json).expect("Failed to deserialize proof JSON");
        println!("Deserialized receipt: {:#?}", proof);
    }

    #[test]
    fn test_deserialize_zkvm_receipt() {
        // let file_name = format!("../../../boundless_receipt_test.json");
        let file_name = format!("../../../boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Proof = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let zkvm_receipt: ZkvmReceipt = serde_json::from_str(&proof.quote.unwrap()).unwrap();
        println!("Deserialized zkvm receipt: {:#?}", zkvm_receipt);
    }

    #[tokio::test]
    async fn test_run_proof_aggregation() {
        env_logger::init();

        let file_name = format!("../../../boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Proof = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let input: AggregationGuestInput = AggregationGuestInput {
            proofs: vec![proof],
        };
        let output: AggregationGuestOutput = AggregationGuestOutput { hash: B256::ZERO };
        let config = ProverConfig::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover
            .aggregate(input, &output, &config, None)
            .await
            .unwrap();
        println!("proof: {:?}", proof);
    }
}
