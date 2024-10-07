#![cfg(feature = "enable")]
#![feature(iter_advance_by)]

use once_cell::sync::Lazy;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestInput, GuestOutput,
        ZkAggregationGuestInput,
    },
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
    Measurement,
};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sp1_sdk::{
    action,
    network::client::NetworkClient,
    proto::network::{ProofMode, UnclaimReason},
    SP1Proof, SP1ProofWithPublicValues, SP1VerifyingKey,
};
use sp1_sdk::{HashableKey, ProverClient, SP1Stdin};
use std::{
    borrow::BorrowMut,
    env, fs,
    path::{Path, PathBuf},
};
use tracing::{debug, error, info};

pub const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");
pub const AGGREGATION_ELF: &[u8] = include_bytes!("../../guest/elf/sp1-aggregation");
const SP1_PROVER_CODE: u8 = 1;
static FIXTURE_PATH: Lazy<PathBuf> =
    Lazy::new(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../contracts/src/fixtures/"));
static CONTRACT_PATH: Lazy<PathBuf> =
    Lazy::new(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../contracts/src/exports/"));

pub static VERIFIER: Lazy<Result<PathBuf, ProverError>> = Lazy::new(init_verifier);
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sp1Param {
    #[serde(default = "RecursionMode::default")]
    pub recursion: RecursionMode,
    pub prover: Option<ProverMode>,
    #[serde(default = "bool::default")]
    pub verify: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RecursionMode {
    /// The proof mode for an SP1 core proof.
    Core,
    /// The proof mode for a compressed proof.
    Compressed,
    /// The proof mode for a PlonK proof.
    #[default]
    Plonk,
}

impl From<RecursionMode> for ProofMode {
    fn from(value: RecursionMode) -> Self {
        match value {
            RecursionMode::Core => ProofMode::Core,
            RecursionMode::Compressed => ProofMode::Compressed,
            RecursionMode::Plonk => ProofMode::Plonk,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProverMode {
    Mock,
    Local,
    Network,
}

impl From<Sp1Response> for Proof {
    fn from(value: Sp1Response) -> Self {
        Self {
            proof: value.proof,
            quote: value
                .sp1_proof
                .as_ref()
                .map(|p| serde_json::to_string(&p.proof).unwrap()),
            input: value
                .sp1_proof
                .as_ref()
                .map(|p| B256::from_slice(p.public_values.as_slice())),
            uuid: value.vkey.map(|v| serde_json::to_string(&v).unwrap()),
            kzg_proof: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: Option<String>,
    /// for aggregation
    pub sp1_proof: Option<SP1ProofWithPublicValues>,
    pub vkey: Option<SP1VerifyingKey>,
}

pub struct Sp1Prover;

impl Prover for Sp1Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();
        let mode = param.prover.clone().unwrap_or_else(get_env_mock);

        println!("param: {param:?}");

        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = param
            .prover
            .map(|mode| match mode {
                ProverMode::Mock => ProverClient::mock(),
                ProverMode::Local => ProverClient::local(),
                ProverMode::Network => ProverClient::network(),
            })
            .unwrap_or_else(ProverClient::new);

        let (pk, vk) = client.setup(ELF);
        info!(
            "Sp1 Prover: block {:?} with vk {:?}",
            output.header.number,
            vk.bytes32()
        );

        let prove_action = action::Prove::new(client.prover.as_ref(), &pk, stdin.clone());
        let prove_result = if !matches!(mode, ProverMode::Network) {
            tracing::debug!("Proving locally with recursion mode: {:?}", param.recursion);
            match param.recursion {
                RecursionMode::Core => prove_action.run(),
                RecursionMode::Compressed => prove_action.compressed().run(),
                RecursionMode::Plonk => prove_action.plonk().run(),
            }
            .map_err(|e| ProverError::GuestError(format!("Sp1: local proving failed: {e}")))?
        } else {
            let network_prover = sp1_sdk::NetworkProver::new();

            let proof_id = network_prover
                .request_proof(ELF, stdin, param.recursion.clone().into())
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Sp1: requesting proof failed: {e}"))
                })?;
            if let Some(id_store) = id_store {
                id_store
                    .store_id(
                        (input.chain_spec.chain_id, output.hash, SP1_PROVER_CODE),
                        proof_id.clone(),
                    )
                    .await?;
            }
            info!(
                "Sp1 Prover: block {:?} - proof id {proof_id:?}",
                output.header.number
            );
            network_prover
                .wait_proof::<sp1_sdk::SP1ProofWithPublicValues>(&proof_id, None)
                .await
                .map_err(|e| ProverError::GuestError(format!("Sp1: network proof failed {e:?}")))?
        };

        let proof_bytes = match param.recursion {
            RecursionMode::Compressed => {
                info!("Compressed proof is used in aggregation mode only");
                vec![]
            }
            _ => prove_result.bytes(),
        };
        if param.verify {
            let time = Measurement::start("verify", false);
            let pi_hash = prove_result
                .clone()
                .borrow_mut()
                .public_values
                .read::<[u8; 32]>();
            let fixture = RaikoProofFixture {
                vkey: vk.bytes32().to_string(),
                public_values: B256::from_slice(&pi_hash).to_string(),
                proof: reth_primitives::hex::encode_prefixed(&proof_bytes),
            };

            verify_sol(&fixture)?;
            time.stop_with("==> Verification complete");
        }

        let proof_string = (!proof_bytes.is_empty()).then_some(
            // 0x + 64 bytes of the vkey + the proof
            // vkey itself contains 0x prefix
            format!(
                "{}{}",
                vk.bytes32(),
                reth_primitives::hex::encode(proof_bytes)
            ),
        );

        info!(
            "Sp1 Prover: block {:?} completed! proof: {proof_string:?}",
            output.header.number,
        );
        Ok::<_, ProverError>(
            Sp1Response {
                proof: proof_string,
                sp1_proof: Some(prove_result),
                vkey: Some(vk),
            }
            .into(),
        )
    }

    async fn cancel(key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        let proof_id = match id_store.read_id(key).await {
            Ok(proof_id) => proof_id,
            Err(e) => {
                if e.to_string().contains("No data for query") {
                    return Ok(());
                } else {
                    return Err(ProverError::GuestError(e.to_string()));
                }
            }
        };
        let private_key = env::var("SP1_PRIVATE_KEY").map_err(|_| {
            ProverError::GuestError("SP1_PRIVATE_KEY must be set for remote proving".to_owned())
        })?;
        let network_client = NetworkClient::new(&private_key);
        network_client
            .unclaim_proof(proof_id, UnclaimReason::Abandoned, "".to_owned())
            .await
            .map_err(|_| ProverError::GuestError("Sp1: couldn't unclaim proof".to_owned()))?;
        id_store.remove_id(key).await?;
        Ok(())
    }

    async fn aggregate(
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();
        let mode = param.prover.clone().unwrap_or_else(get_env_mock);

        info!("aggregate proof with param: {param:?}");

        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .map(|proof| proof.input.unwrap())
            .collect::<Vec<_>>();
        let block_proof_vk = serde_json::from_str::<SP1VerifyingKey>(
            &input.proofs.first().unwrap().uuid.clone().unwrap(),
        )
        .map_err(|e| ProverError::GuestError(format!("Failed to parse SP1 vk: {e}")))?;
        let stark_vk = block_proof_vk.vk.clone();
        let image_id = block_proof_vk.hash_u32();
        let aggregation_input = ZkAggregationGuestInput {
            image_id: image_id,
            block_inputs,
        };
        info!(
            "Aggregating {:?} proofs with input: {aggregation_input:?}",
            input.proofs.len(),
        );

        let mut stdin = SP1Stdin::new();
        stdin.write(&aggregation_input);
        for proof in input.proofs.iter() {
            let sp1_proof = serde_json::from_str::<SP1Proof>(&proof.quote.clone().unwrap())
                .map_err(|e| ProverError::GuestError(format!("Failed to parse SP1 proof: {e}")))?;
            match sp1_proof {
                SP1Proof::Compressed(block_proof) => {
                    stdin.write_proof(block_proof.into(), stark_vk.clone());
                }
                _ => {
                    error!("unsupported proof type for aggregation: {sp1_proof:?}");
                }
            }
        }

        // Generate the proof for the given program.
        let client = param
            .prover
            .map(|mode| match mode {
                ProverMode::Mock => ProverClient::mock(),
                ProverMode::Local => ProverClient::local(),
                ProverMode::Network => ProverClient::network(),
            })
            .unwrap_or_else(ProverClient::new);

        let (pk, vk) = client.setup(AGGREGATION_ELF);
        info!(
            "sp1 aggregate: {:?} based {:?} blocks with vk {:?}",
            reth_primitives::hex::encode_prefixed(stark_vk.hash_bytes()),
            input.proofs.len(),
            vk.bytes32()
        );

        let prove_result = client
            .prove(&pk, stdin)
            .plonk()
            .run()
            .expect("proving failed");

        let proof_bytes = prove_result.bytes();
        if param.verify {
            let time = Measurement::start("verify", false);
            let aggregation_pi = prove_result.clone().borrow_mut().public_values.raw();
            let fixture = RaikoProofFixture {
                vkey: vk.bytes32().to_string(),
                public_values: reth_primitives::hex::encode_prefixed(&aggregation_pi),
                proof: reth_primitives::hex::encode_prefixed(&proof_bytes),
            };

            verify_sol(&fixture)?;
            time.stop_with("==> Verification complete");
        }

        let proof = (!proof_bytes.is_empty()).then_some(
            // 0x + 64 bytes of the vkey + the proof
            // vkey itself contains 0x prefix
            format!(
                "{}{}{}",
                vk.bytes32(),
                reth_primitives::hex::encode(stark_vk.hash_bytes()),
                reth_primitives::hex::encode(proof_bytes)
            ),
        );

        Ok::<_, ProverError>(
            Sp1Response {
                proof: proof,
                sp1_proof: None,
                vkey: None,
            }
            .into(),
        )
    }
}

fn get_env_mock() -> ProverMode {
    match env::var("SP1_PROVER")
        .unwrap_or("local".to_string())
        .to_lowercase()
        .as_str()
    {
        "mock" => ProverMode::Mock,
        "local" => ProverMode::Local,
        "network" => ProverMode::Network,
        _ => ProverMode::Local,
    }
}

fn init_verifier() -> Result<PathBuf, ProverError> {
    // In cargo run, Cargo sets the working directory to the root of the workspace
    let contract_path = &*CONTRACT_PATH;
    info!("Contract dir: {contract_path:?}");
    let artifacts_dir = sp1_sdk::install::try_install_circuit_artifacts();
    // Create the destination directory if it doesn't exist
    fs::create_dir_all(contract_path)?;

    // Read the entries in the source directory
    for entry in fs::read_dir(artifacts_dir)? {
        let entry = entry?;
        let src = entry.path();

        // Check if the entry is a file and ends with .sol
        if src.is_file() && src.extension().map(|s| s == "sol").unwrap_or(false) {
            let out = contract_path.join(src.file_name().unwrap());
            fs::copy(&src, &out)?;
            println!("Copied: {:?}", src.file_name().unwrap());
        }
    }
    Ok(contract_path.clone())
}

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RaikoProofFixture {
    vkey: String,
    public_values: String,
    proof: String,
}

fn verify_sol(fixture: &RaikoProofFixture) -> ProverResult<()> {
    assert!(VERIFIER.is_ok());
    debug!("===> Fixture: {fixture:#?}");

    // Save the fixture to a file.
    let fixture_path = &*FIXTURE_PATH;
    info!("Writing fixture to: {fixture_path:?}");

    if !fixture_path.exists() {
        std::fs::create_dir_all(fixture_path.clone())
            .map_err(|e| ProverError::GuestError(format!("Failed to create fixture path: {e}")))?;
    }
    std::fs::write(
        fixture_path.join("fixture.json"),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .map_err(|e| ProverError::GuestError(format!("Failed to write fixture: {e}")))?;

    let child = std::process::Command::new("forge")
        .arg("test")
        .current_dir(&*CONTRACT_PATH)
        .stdout(std::process::Stdio::inherit()) // Inherit the parent process' stdout
        .spawn();
    info!("Verification started {child:?}");
    child.map_err(|e| ProverError::GuestError(format!("Failed to run forge: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;
    const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-guest");

    #[test]
    fn test_deserialize_sp1_param() {
        let json = json!(
            {
                "recursion": "core",
                "prover": "network",
                "verify": true
            }
        );
        let param = Sp1Param {
            recursion: RecursionMode::Core,
            prover: Some(ProverMode::Network),
            verify: true,
        };
        let serialized = serde_json::to_value(param).unwrap();
        assert_eq!(json, serialized);

        let deserialized: Sp1Param = serde_json::from_value(serialized).unwrap();
        println!("{json:?} {deserialized:?}");
    }

    #[test]
    fn test_init_verifier() {
        VERIFIER.as_ref().expect("Failed to init verifier");
    }

    #[test]
    fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::new();
        let stdin = SP1Stdin::new();
        let (pk, vk) = client.setup(TEST_ELF);
        let proof = client.prove(&pk, stdin).run().unwrap();
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");
    }

    #[ignore = "This is for docker image build only"]
    #[test]
    fn test_show_sp1_elf_vk() {
        let client = ProverClient::new();
        let (_pk, vk) = client.setup(ELF);
        println!("SP1 ELF VK: {:?}", vk.bytes32());
    }
}
