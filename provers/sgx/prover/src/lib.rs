#![cfg(feature = "enable")]
use std::{
    env,
    fs::{copy, create_dir_all, remove_file},
    path::PathBuf,
    process::{Command as StdCommand, Output, Stdio},
    str,
};

use alloy_sol_types::SolValue;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverConfig, ProverError, ProverResult},
};
use raiko_primitives::{keccak::keccak, B256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::serde_as;
use tokio::process::Command;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SgxParam {
    pub instance_id: u64,
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
}

pub const ELF_NAME: &str = "sgx-guest";
pub const CONFIG: &str = "../../provers/sgx/config";

pub struct SgxProver;

impl Prover for SgxProver {
    async fn run(
        input: GuestInput,
        _output: GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let config = SgxParam::deserialize(config.get("sgx").unwrap()).unwrap();

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

        // Prepare prerequisites if running in direct mode. For SGX mode, we assume they are
        // already prepared by the Docker image.
        let cur_dir = if direct_mode {
            let dir = env::current_exe()
                .expect("Fail to get current directory")
                .parent()
                .unwrap()
                .to_path_buf();
            println!("Current directory: {:?}\n", dir);
            setup(dir.clone()).await?;
            dir
        } else {
            PathBuf::from("/opt/raiko/provers/sgx")
        };

        // The Gramine command - 'gramine-sgx' for SGX environment, or 'gramine-direct' for
        // testing in non-SGX environment (a.k.a. simulation mode).
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

        if config.bootstrap {
            bootstrap(cur_dir.clone(), gramine_cmd()).await?;
        }

        // Prove: run for each block
        let sgx_proof = if config.prove {
            prove(gramine_cmd(), input.clone(), config.instance_id).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        to_proof(sgx_proof)
    }

    fn instance_hash(pi: ProtocolInstance) -> B256 {
        let data = (
            "VERIFY_PROOF",
            pi.chain_id,
            pi.transition.clone(),
            // new_pubkey, TODO(cecilia)
            pi.prover,
            pi.meta_hash(),
        )
            .abi_encode();

        keccak(data).into()
    }
}

// This function prepares the working directory for the SGX prover running in testing
// (direct/simulation) mode. It is not applicable in hardware mode.
async fn setup(dir: PathBuf) -> ProverResult<()> {
    // Create required directories
    let directories = ["secrets", "config"];
    for cur_dir in directories {
        create_dir_all(dir.join(cur_dir)).unwrap();
    }
    let gramine_manifest_template = dir.join(CONFIG).join("sgx-guest.local.manifest.template");

    // Copy dummy files in direct mode
    let files = ["attestation_type", "quote", "user_report_data"];
    for file in files {
        copy(
            dir.join(CONFIG).join("dummy_data").join(file),
            dir.join(file),
        )
        .unwrap();
    }

    // Generate Gramine's manifest
    let mut cmd = Command::new("gramine-manifest");
    let output = cmd
        .current_dir(dir.clone())
        .arg("-Dlog_level=error")
        .arg("-Darch_libdir=/lib/x86_64-linux-gnu/")
        .arg("-Ddirect_mode=1")
        .arg(gramine_manifest_template)
        .arg("sgx-guest.manifest")
        .output()
        .await
        .map_err(|e| format!("Could not generate manifest: {}", e))?;

    print_output(&output, "Generate manifest");

    Ok(())
}

async fn bootstrap(dir: PathBuf, mut gramine_cmd: StdCommand) -> ProverResult<(), String> {
    tokio::task::spawn_blocking(move || {
        // Bootstrap with new private key for signing proofs
        // First delete the private key if it already exists
        let path = dir.join("secrets").join("priv.key");
        if path.exists() {
            if let Err(e) = remove_file(&path) {
                println!("Error deleting file: {}", e);
            }
        }
        let output = gramine_cmd
            .arg("bootstrap")
            .output()
            .map_err(|e| handle_gramine_error("Could not run SGX guest bootstrap", e))?;
        print_output(&output, "SGX bootstrap");

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
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
            .map_err(|e| format!("Could not spawn gramine cmd: {}", e))?;
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        bincode::serialize_into(stdin, &input).expect("Unable to serialize input");

        let output = child
            .wait_with_output()
            .map_err(|e| handle_gramine_error("Could not run SGX guest prover", e))?;
        print_output(&output, "Sgx execution");
        if !output.status.success() {
            return ProverResult::Err(ProverError::GuestError(output.status.to_string()));
        }
        Ok(parse_sgx_result(output.stdout)?)
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
    })
}

fn handle_gramine_error(context: &str, err: std::io::Error) -> String {
    if let std::io::ErrorKind::NotFound = err.kind() {
        format!(
            "gramine could not be found, please install gramine first. ({})",
            err
        )
    } else {
        format!("{}: {}", context, err)
    }
}

fn print_output(output: &Output, name: &str) {
    println!(
        "{} stderr: {}",
        name,
        str::from_utf8(&output.stderr).unwrap()
    );
    println!(
        "{} stdout: {}",
        name,
        str::from_utf8(&output.stdout).unwrap()
    );
}
