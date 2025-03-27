#![cfg(feature = "enable")]

use std::{
    collections::HashMap,
    env,
    fs::{copy, create_dir_all, remove_file},
    path::{Path, PathBuf},
    process::{Command as StdCommand, Output, Stdio},
    str::{self, FromStr},
};

use once_cell::sync::Lazy;
use raiko_lib::{
    consts::SpecId,
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, RawAggregationGuestInput, RawProof,
    },
    primitives::B256,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::serde_as;
use tokio::{process::Command, sync::OnceCell};

pub use crate::sgx_register_utils::{
    get_instance_id, register_sgx_instance, remove_instance_id, set_instance_id, ForkRegisterId,
};

pub const PRIV_KEY_FILENAME: &str = "priv.key";

// to register the instance id
mod sgx_register_utils;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SgxParam {
    pub instance_ids: HashMap<SpecId, u64>,
    pub setup: bool,
    pub bootstrap: bool,
    pub prove: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
    pub input: B256,
}

impl From<SgxResponse> for Proof {
    fn from(value: SgxResponse) -> Self {
        Self {
            proof: Some(value.proof),
            input: Some(value.input),
            quote: Some(value.quote),
            uuid: None,
            kzg_proof: None,
        }
    }
}

pub const ELF_NAME: &str = "sgx-guest";
pub const GAIKO_ELF_NAME: &str = "gaiko";
#[cfg(feature = "docker_build")]
pub const CONFIG: &str = "../provers/sgx/config";
#[cfg(not(feature = "docker_build"))]
pub const CONFIG: &str = "../../provers/sgx/config";
static GRAMINE_MANIFEST_TEMPLATE: Lazy<OnceCell<PathBuf>> = Lazy::new(OnceCell::new);
static PRIVATE_KEY: Lazy<OnceCell<PathBuf>> = Lazy::new(OnceCell::new);

pub struct SgxProver;

impl Prover for SgxProver {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param = SgxParam::deserialize(config.get("sgx").unwrap()).unwrap();

        // Support both SGX and the direct backend for testing
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        println!(
            "WARNING: running SGX in {} mode!",
            if direct_mode {
                "direct (a.k.a. simulation)"
            } else {
                "hardware"
            }
        );

        // The working directory
        let mut cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir.parent().unwrap().to_path_buf();
        }

        println!("Current directory: {cur_dir:?}\n");
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async {
                cur_dir
                    .join(CONFIG)
                    .join("sgx-guest.local.manifest.template")
            })
            .await;

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> StdCommand {
            let mut cmd = if direct_mode {
                StdCommand::new("gramine-direct")
            } else {
                let mut cmd = StdCommand::new("sudo");
                cmd.arg("gramine-sgx");
                cmd
            };
            cmd.current_dir(&cur_dir).arg(ELF_NAME);
            cmd
        };

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            setup(&cur_dir, direct_mode).await?;
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(cur_dir.clone().join("secrets"), gramine_cmd()).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input, &sgx_param)?;
            sgx_proof = prove(gramine_cmd(), input.clone(), instance_id).await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn aggregate(
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param = SgxParam::deserialize(config.get("sgx").unwrap()).unwrap();

        // Support both SGX and the direct backend for testing
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        println!(
            "WARNING: running SGX in {} mode!",
            if direct_mode {
                "direct (a.k.a. simulation)"
            } else {
                "hardware"
            }
        );

        // The working directory
        let mut cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir.parent().unwrap().to_path_buf();
        }

        println!("Current directory: {cur_dir:?}\n");
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async {
                cur_dir
                    .join(CONFIG)
                    .join("sgx-guest.local.manifest.template")
            })
            .await;

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> StdCommand {
            let mut cmd = if direct_mode {
                StdCommand::new("gramine-direct")
            } else {
                let mut cmd = StdCommand::new("sudo");
                cmd.arg("gramine-sgx");
                cmd
            };
            cmd.current_dir(&cur_dir).arg(ELF_NAME);
            cmd
        };

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            setup(&cur_dir, direct_mode).await?;
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(cur_dir.clone().join("secrets"), gramine_cmd()).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            sgx_proof = aggregate(gramine_cmd(), input.clone()).await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn cancel(_proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    async fn batch_run(
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param = SgxParam::deserialize(config.get("sgx").unwrap()).unwrap();

        let is_pivot = match env::var("PIVOT") {
            Ok(value) => value == "true",
            Err(_) => false,
        };

        // Support both SGX and the direct backend for testing
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        println!(
            "WARNING: running SGX in {} mode!",
            if direct_mode {
                "direct (a.k.a. simulation)"
            } else {
                "hardware"
            }
        );

        // The working directory
        let mut cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir.parent().unwrap().to_path_buf();
        }

        println!("Current directory: {cur_dir:?}\n");
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async {
                cur_dir
                    .join(CONFIG)
                    .join("sgx-guest.local.manifest.template")
            })
            .await;

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> StdCommand {
            let (mut cmd, elf) = if direct_mode {
                (StdCommand::new("gramine-direct"), Some(ELF_NAME))
            } else if is_pivot {
                let mut cmd = StdCommand::new(cur_dir.join(GAIKO_ELF_NAME));
                (cmd, None)
            } else {
                let mut cmd = StdCommand::new("sudo");
                cmd.arg("gramine-sgx");
                (cmd, Some(ELF_NAME))
            };
            if let Some(elf) = elf {
                cmd.current_dir(&cur_dir).arg(elf);
            }
            cmd
        };

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup && !is_pivot {
            setup(&cur_dir, direct_mode).await?;
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(cur_dir.clone().join("secrets"), gramine_cmd()).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input.inputs[0], &sgx_param)?;
            sgx_proof = batch_prove(gramine_cmd(), input.clone(), instance_id).await
        }

        sgx_proof.map(|r| r.into())
    }
}

async fn setup(cur_dir: &Path, direct_mode: bool) -> ProverResult<(), String> {
    // Create required directories
    let directories = ["secrets", "config"];
    for dir in directories {
        create_dir_all(cur_dir.join(dir)).unwrap();
    }
    if direct_mode {
        // Copy dummy files in direct mode
        let files = ["attestation_type", "quote", "user_report_data"];
        for file in files {
            copy(
                cur_dir.join(CONFIG).join("dummy_data").join(file),
                cur_dir.join(file),
            )
            .unwrap();
        }
    }

    // Generate the manifest
    let mut cmd = Command::new("gramine-manifest");
    let output = cmd
        .current_dir(cur_dir)
        .arg("-Dlog_level=error")
        .arg("-Darch_libdir=/lib/x86_64-linux-gnu/")
        .arg(format!(
            "-Ddirect_mode={}",
            if direct_mode { "1" } else { "0" }
        ))
        .arg(GRAMINE_MANIFEST_TEMPLATE.get().unwrap())
        .arg("sgx-guest.manifest")
        .output()
        .await
        .map_err(|e| handle_gramine_error("Could not generate manfifest", e))?;
    handle_output(&output, "SGX generate manifest")?;

    if !direct_mode {
        // Generate a private key
        let mut cmd = Command::new("gramine-sgx-gen-private-key");
        let output = cmd
            .current_dir(cur_dir)
            .arg("-f")
            .output()
            .await
            .map_err(|e| handle_gramine_error("Could not generate SGX private key", e))?;
        handle_output(&output, "SGX private key")?;

        // Sign the manifest
        let mut cmd = Command::new("gramine-sgx-sign");
        let output = cmd
            .current_dir(cur_dir)
            .arg("--manifest")
            .arg("sgx-guest.manifest")
            .arg("--output")
            .arg("sgx-guest.manifest.sgx")
            .output()
            .await
            .map_err(|e| handle_gramine_error("Could not sign manfifest", e))?;
        handle_output(&output, "SGX manifest sign")?;
    }

    Ok(())
}

pub async fn check_bootstrap(
    secret_dir: PathBuf,
    mut gramine_cmd: StdCommand,
) -> ProverResult<(), ProverError> {
    tokio::task::spawn_blocking(move || {
        // Check if the private key exists
        let path = secret_dir.join(PRIV_KEY_FILENAME);
        if !path.exists() {
            Err(ProverError::GuestError(
                "Private key does not exist".to_string(),
            ))
        } else {
            // Check if the private key is valid
            let output = gramine_cmd.arg("check").output().map_err(|e| {
                ProverError::GuestError(handle_gramine_error(
                    "Could not run SGX guest bootstrap",
                    e,
                ))
            })?;
            handle_output(&output, "SGX check bootstrap")?;
            Ok(())
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

pub async fn bootstrap(
    secret_dir: PathBuf,
    mut gramine_cmd: StdCommand,
) -> ProverResult<SgxResponse, ProverError> {
    tokio::task::spawn_blocking(move || {
        // Bootstrap with new private key for signing proofs
        // First delete the private key if it already exists
        let path = secret_dir.join(PRIV_KEY_FILENAME);
        if path.exists() {
            if let Err(e) = remove_file(&path) {
                println!("Error deleting file: {e}");
            }
        }
        let output = gramine_cmd
            .arg("bootstrap")
            .output()
            .map_err(|e| handle_gramine_error("Could not run SGX guest bootstrap", e))?;
        handle_output(&output, "SGX bootstrap")?;

        Ok(parse_sgx_result(output.stdout)?)
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn prove(
    mut gramine_cmd: StdCommand,
    input: GuestInput,
    instance_id: u64,
) -> ProverResult<SgxResponse, ProverError> {
    tokio::task::spawn_blocking(move || {
        let mut child = gramine_cmd
            .arg("one-shot")
            .arg("--sgx-instance-id")
            .arg(instance_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not spawn gramine cmd: {e}"))?;
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        let input_success = bincode::serialize_into(stdin, &input);
        let output_success = child.wait_with_output();

        match (input_success, output_success) {
            (Ok(_), Ok(output)) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            (Err(i), output_success) => Err(ProverError::GuestError(format!(
                "Can not serialize input for SGX {i}, output is {output_success:?}"
            ))),
            (Ok(_), Err(output_err)) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn batch_prove(
    mut gramine_cmd: StdCommand,
    input: GuestBatchInput,
    instance_id: u64,
) -> ProverResult<SgxResponse, ProverError> {
    tokio::task::spawn_blocking(move || {
        let mut child = gramine_cmd
            .arg("one-batch-shot")
            .arg("--sgx-instance-id")
            .arg(instance_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not spawn gramine cmd: {e}"))?;
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        let input_success = bincode::serialize_into(stdin, &input);
        let output_success = child.wait_with_output();

        match (input_success, output_success) {
            (Ok(_), Ok(output)) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            (Err(i), output_success) => Err(ProverError::GuestError(format!(
                "Can not serialize input for SGX {i}, output is {output_success:?}"
            ))),
            (Ok(_), Err(output_err)) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn aggregate(
    mut gramine_cmd: StdCommand,
    input: AggregationGuestInput,
) -> ProverResult<SgxResponse, ProverError> {
    // Extract the useful parts of the proof here so the guest doesn't have to do it
    let raw_input = RawAggregationGuestInput {
        proofs: input
            .proofs
            .iter()
            .map(|proof| RawProof {
                input: proof.clone().input.unwrap(),
                proof: hex::decode(&proof.clone().proof.unwrap()[2..]).unwrap(),
            })
            .collect(),
    };
    // Extract the instance id from the first proof
    let instance_id = {
        let mut instance_id_bytes = [0u8; 4];
        instance_id_bytes[0..4].copy_from_slice(&raw_input.proofs[0].proof.clone()[0..4]);
        u32::from_be_bytes(instance_id_bytes)
    };

    tokio::task::spawn_blocking(move || {
        let mut child = gramine_cmd
            .arg("aggregate")
            .arg("--sgx-instance-id")
            .arg(instance_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not spawn gramine cmd: {e}"))?;
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        let input_success = bincode::serialize_into(stdin, &raw_input);
        let output_success = child.wait_with_output();

        match (input_success, output_success) {
            (Ok(_), Ok(output)) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            (Err(i), output_success) => Err(ProverError::GuestError(format!(
                "Can not serialize input for SGX {i}, output is {output_success:?}"
            ))),
            (Ok(_), Err(output_err)) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

fn parse_sgx_result(output: Vec<u8>) -> ProverResult<SgxResponse, String> {
    let mut json_value: Option<Value> = None;
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;

    for line in output.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
            json_value = Some(value);
            break;
        }
    }
    let extract_field = |field| {
        json_value
            .as_ref()
            .and_then(|json| json.get(field).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string()
    };

    Ok(SgxResponse {
        proof: extract_field("proof"),
        quote: extract_field("quote"),
        input: B256::from_str(&extract_field("input")).unwrap_or_default(),
    })
}

fn handle_gramine_error(context: &str, err: std::io::Error) -> String {
    if let std::io::ErrorKind::NotFound = err.kind() {
        format!("gramine could not be found, please install gramine first. ({err})")
    } else {
        format!("{context}: {err}")
    }
}

fn handle_output(output: &Output, name: &str) -> ProverResult<(), String> {
    println!("{name} stderr: {}", str::from_utf8(&output.stderr).unwrap());
    println!("{name} stdout: {}", str::from_utf8(&output.stdout).unwrap());
    if !output.status.success() {
        return Err(format!(
            "{name} encountered an error ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(())
}

pub fn get_instance_id_from_params(input: &GuestInput, sgx_param: &SgxParam) -> ProverResult<u64> {
    let spec_id = input
        .chain_spec
        .active_fork(input.block.number, input.block.timestamp)
        .map_err(|e| ProverError::GuestError(e.to_string()))?;
    sgx_param
        .instance_ids
        .get(&spec_id)
        .cloned()
        .ok_or_else(|| {
            ProverError::GuestError(format!("No instance id found for spec id: {:?}", spec_id))
        })
}
