use log::{debug, error, info, warn};
use raiko_lib::{
    primitives::keccak::keccak,
    prover::{IdWrite, ProofKey},
};
use risc0_zkvm::{
    compute_image_id, is_dev_mode, serde::to_vec, sha::Digest, Assumption, ExecutorEnv,
    ExecutorImpl, Receipt,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fmt::Debug,
    fs,
    path::{Path, PathBuf},
};

use crate::Risc0Param;

pub async fn verify_bonsai_receipt<O: Eq + Debug + DeserializeOwned>(
    image_id: Digest,
    expected_output: &O,
    uuid: String,
    max_retries: usize,
) -> anyhow::Result<(String, Receipt)> {
    info!("Tracking receipt uuid: {uuid}");
    let session = bonsai_sdk::alpha::SessionId { uuid };

    loop {
        let mut res = None;
        for attempt in 1..=max_retries {
            let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;

            match session.status(&client) {
                Ok(response) => {
                    res = Some(response);
                    break;
                }
                Err(err) => {
                    if attempt == max_retries {
                        anyhow::bail!(err);
                    }
                    warn!("Attempt {attempt}/{max_retries} for session status request: {err:?}");
                    std::thread::sleep(std::time::Duration::from_secs(15));
                    continue;
                }
            }
        }

        let res = res.unwrap();

        if res.status == "RUNNING" {
            info!(
                "Current status: {} - state: {} - continue polling...",
                res.status,
                res.state.unwrap_or_default()
            );
            std::thread::sleep(std::time::Duration::from_secs(15));
        } else if res.status == "SUCCEEDED" {
            // Download the receipt, containing the output
            let receipt_url = res
                .receipt_url
                .expect("API error, missing receipt on completed session");
            let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
            let receipt_buf = client.download(&receipt_url)?;
            let receipt: Receipt = bincode::deserialize(&receipt_buf)?;
            receipt
                .verify(image_id)
                .expect("Receipt verification failed");
            // verify output
            let receipt_output: O = receipt.journal.decode().unwrap();
            if expected_output == &receipt_output {
                info!("Receipt validated!");
            } else {
                error!(
                    "Output mismatch! Receipt: {receipt_output:?}, expected: {expected_output:?}",
                );
            }
            return Ok((session.uuid, receipt));
        } else {
            panic!(
                "Workflow exited: {} - | err: {}",
                res.status,
                res.error_msg.unwrap_or_default()
            );
        }
    }
}

pub async fn maybe_prove<I: Serialize, O: Eq + Debug + Serialize + DeserializeOwned>(
    param: &Risc0Param,
    encoded_input: Vec<u32>,
    elf: &[u8],
    expected_output: &O,
    assumptions: (Vec<Assumption>, Vec<String>),
    proof_key: ProofKey,
    id_store: &mut Option<&mut dyn IdWrite>,
) -> Option<(String, Receipt)> {
    let (assumption_instances, assumption_uuids) = assumptions;

    let encoded_output =
        to_vec(expected_output).expect("Could not serialize expected proving output!");
    let computed_image_id = compute_image_id(elf).expect("Failed to compute elf image id!");

    let receipt_label = format!(
        "{}-{}",
        hex::encode(computed_image_id),
        hex::encode(keccak(bytemuck::cast_slice(&encoded_output)))
    );

    // get receipt
    let (mut receipt_uuid, receipt, cached) =
        if let Ok(Some(cached_data)) = load_receipt(&receipt_label) {
            info!("Loaded locally cached stark receipt {receipt_label:?}");
            (cached_data.0, cached_data.1, true)
        } else if param.bonsai {
            // query bonsai service until it works
            loop {
                match prove_bonsai(
                    encoded_input.clone(),
                    elf,
                    expected_output,
                    assumption_uuids.clone(),
                    proof_key,
                    id_store,
                )
                .await
                {
                    Ok((receipt_uuid, receipt)) => {
                        break (receipt_uuid, receipt, false);
                    }
                    Err(err) => {
                        warn!("Failed to prove on Bonsai: {err:?}");
                        std::thread::sleep(std::time::Duration::from_secs(15));
                    }
                }
            }
        } else {
            // run prover
            info!("start running local prover");
            (
                Default::default(),
                prove_locally(
                    param.execution_po2,
                    encoded_input,
                    elf,
                    assumption_instances,
                    param.profile,
                ),
                false,
            )
        };

    info!("receipt: {receipt:?}");
    info!("journal: {:?}", receipt.journal);

    // verify output
    let output_guest: O = receipt.journal.decode().unwrap();
    if expected_output == &output_guest {
        info!("Prover succeeded");
    } else {
        error!("Output mismatch! Prover: {output_guest:?}, expected: {expected_output:?}");
    }

    // upload receipt to bonsai
    if param.bonsai && receipt_uuid.is_empty() {
        info!("Uploading cached receipt without UUID to Bonsai.");
        receipt_uuid = upload_receipt(&receipt)
            .await
            .expect("Failed to upload cached receipt to Bonsai");
    }

    let result = (receipt_uuid, receipt);

    // save receipt
    if !cached {
        save_receipt(&receipt_label, &result);
    }

    // return result
    Some(result)
}

pub async fn upload_receipt(receipt: &Receipt) -> anyhow::Result<String> {
    let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
    Ok(client.upload_receipt(bincode::serialize(receipt)?)?)
}

pub async fn cancel_proof(uuid: String) -> anyhow::Result<()> {
    let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
    let session = bonsai_sdk::alpha::SessionId { uuid };
    session.stop(&client)?;
    Ok(())
}

pub async fn prove_bonsai<O: Eq + Debug + DeserializeOwned>(
    encoded_input: Vec<u32>,
    elf: &[u8],
    expected_output: &O,
    assumption_uuids: Vec<String>,
    proof_key: ProofKey,
    id_store: &mut Option<&mut dyn IdWrite>,
) -> anyhow::Result<(String, Receipt)> {
    info!("Proving on Bonsai");
    // Compute the image_id, then upload the ELF with the image_id as its key.
    let image_id = risc0_zkvm::compute_image_id(elf)?;
    let encoded_image_id = hex::encode(image_id);
    // Prepare input data
    let input_data = bytemuck::cast_slice(&encoded_input).to_vec();

    let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
    client.upload_img(&encoded_image_id, elf.to_vec())?;
    // upload input
    let input_id = client.upload_input(input_data.clone())?;

    let session = client.create_session(
        encoded_image_id.clone(),
        input_id.clone(),
        assumption_uuids.clone(),
    )?;

    if let Some(id_store) = id_store {
        id_store.store_id(proof_key, session.uuid.clone())?;
    }

    verify_bonsai_receipt(image_id, expected_output, session.uuid.clone(), 8).await
}

/// Prove the given ELF locally with the given input and assumptions. The segments are
/// stored in a temporary directory, to allow for proofs larger than the available memory.
pub fn prove_locally(
    segment_limit_po2: u32,
    encoded_input: Vec<u32>,
    elf: &[u8],
    assumptions: Vec<Assumption>,
    profile: bool,
) -> Receipt {
    debug!("Proving with segment_limit_po2 = {segment_limit_po2:?}");
    debug!(
        "Input size: {} words ( {} MB )",
        encoded_input.len(),
        encoded_input.len() * 4 / 1_000_000
    );

    info!("Running the prover...");
    let session = {
        let mut env_builder = ExecutorEnv::builder();
        env_builder
            .session_limit(None)
            .segment_limit_po2(segment_limit_po2)
            .write_slice(&encoded_input);

        if profile {
            info!("Profiling enabled.");
            env_builder.enable_profiler("profile_r0_local.pb");
        }

        for assumption in assumptions {
            env_builder.add_assumption(assumption);
        }

        let segment_dir = PathBuf::from("/tmp/risc0-cache");
        if segment_dir.exists() {
            fs::remove_dir_all(segment_dir.clone()).unwrap();
        }
        fs::create_dir(segment_dir.clone()).unwrap();
        let env = env_builder.segment_path(segment_dir).build().unwrap();
        let mut exec = ExecutorImpl::from_elf(env, elf).unwrap();

        exec.run().unwrap()
    };
    session.prove().unwrap().receipt
}

pub fn load_receipt<T: serde::de::DeserializeOwned>(
    file_name: &String,
) -> anyhow::Result<Option<(String, T)>> {
    if is_dev_mode() {
        // Nothing to load
        return Ok(None);
    }

    let receipt_serialized = match fs::read(zkp_cache_path(file_name)) {
        Ok(receipt_serialized) => receipt_serialized,
        Err(err) => {
            debug!("Could not load cached receipt with label: {}", &file_name);
            debug!("{err:?}");
            return Ok(None);
        }
    };

    Ok(Some(bincode::deserialize(&receipt_serialized)?))
}

pub fn save_receipt<T: serde::Serialize>(receipt_label: &String, receipt_data: &(String, T)) {
    if !is_dev_mode() {
        fs::write(
            zkp_cache_path(receipt_label),
            bincode::serialize(receipt_data).expect("Failed to serialize receipt!"),
        )
        .expect("Failed to save receipt output file.");
    }
}

fn zkp_cache_path(receipt_label: &String) -> String {
    Path::new("/tmp/risc0-cache")
        .join(format!("{receipt_label}.zkp"))
        .to_str()
        .unwrap()
        .to_string()
}
