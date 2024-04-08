#![cfg(feature = "enable")]
use std::{
    env,
    fs::{copy, create_dir_all, remove_file, File},
    path::PathBuf,
    process::Output,
    str,
};

use alloy_sol_types::SolValue;
use once_cell::sync::Lazy;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverConfig, ProverError, ProverResult},
};
use raiko_primitives::{keccak::keccak, B256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::serde_as;
use tokio::{process::Command, sync::OnceCell};

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
pub const INPUT_FILE_NAME: &str = "input.bin";
pub const CONFIG: &str = "../../provers/sgx/config";

static GRAMINE_MANIFEST_TEMPLATE: Lazy<OnceCell<PathBuf>> = Lazy::new(OnceCell::new);
static PRIVATE_KEY: Lazy<OnceCell<PathBuf>> = Lazy::new(OnceCell::new);

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
        // Print a warning when running in direct mode
        if direct_mode {
            println!("WARNING: running SGX in direct mode!");
        }

        // The working directory
        let cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();
        println!("Current directory: {:?}\n", cur_dir);
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join("priv.key") })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async { cur_dir.join(CONFIG).join("raiko-guest.manifest.template") })
            .await;

        // Write the input to a file that will be read by the SGX instance
        let path = cur_dir.join(INPUT_FILE_NAME);
        bincode::serialize_into(File::create(&path).expect("Unable to open file"), &input)
            .expect("Unable to serialize input");

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> Command {
            let mut cmd = if direct_mode {
                Command::new("gramine-direct")
            } else {
                let mut cmd = Command::new("sudo");
                cmd.arg("gramine-sgx");
                cmd
            };
            cmd.current_dir(&cur_dir).arg(ELF_NAME);
            cmd
        };

        // Setup: run this once while setting up your SGX instance
        if config.setup {
            setup(&cur_dir, direct_mode).await?;
        }

        // Boostrap: run this each time a new keypair for proving needs to be generated
        if config.bootstrap {
            bootstrap(&mut gramine_cmd()).await?;
        }

        // Prove: run for each block
        let sgx_proof = if config.prove {
            prove(&mut gramine_cmd(), &cur_dir, input, config.instance_id).await
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

async fn setup(cur_dir: &PathBuf, direct_mode: bool) -> ProverResult<SgxResponse, String> {
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
        .current_dir(cur_dir.clone())
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
        .map_err(|e| format!("Could not generate manfifest: {}", e))?;

    print_output(&output, "Generate manifest");

    if !direct_mode {
        // Generate a private key
        let mut cmd = Command::new("gramine-sgx-gen-private-key");
        cmd.current_dir(cur_dir.clone())
            .arg("-f")
            .output()
            .await
            .map_err(|e| format!("Could not generate SGX private key: {}", e))?;

        // Sign the manifest
        let mut cmd = Command::new("gramine-sgx-sign");
        cmd.current_dir(cur_dir.clone())
            .arg("--manifest")
            .arg("sgx-guest.manifest")
            .arg("--output")
            .arg("sgx-guest.manifest.sgx")
            .output()
            .await
            .map_err(|e| format!("Could not sign manfifest: {}", e))?;
    }

    Ok(SgxResponse::default())
}

async fn bootstrap(gramine_cmd: &mut Command) -> ProverResult<SgxResponse, String> {
    // Bootstrap with new private key for signing proofs
    // First delete the private key if it already exists
    if PRIVATE_KEY.get().unwrap().exists() {
        if let Err(e) = remove_file(PRIVATE_KEY.get().unwrap()) {
            println!("Error deleting file: {}", e);
        }
    }
    let output = gramine_cmd
        .arg("bootstrap")
        .output()
        .await
        .map_err(|e| format!("Could not run SGX guest boostrap: {}", e))?;
    print_output(&output, "Sgx bootstrap");

    Ok(SgxResponse::default())
}

async fn prove(
    gramine_cmd: &mut Command,
    cur_dir: &PathBuf,
    input: GuestInput,
    instance_id: u64,
) -> ProverResult<SgxResponse, ProverError> {
    // Prove
    let output = gramine_cmd
        .arg("one-shot")
        .arg("--sgx-instance-id")
        .arg(instance_id.to_string())
        .output()
        .await
        .map_err(|e| format!("Could not run SGX guest prover: {}", e))?;
    print_output(&output, "Sgx execution");
    if !output.status.success() {
        // inc_sgx_error(req.block_no);
        return ProverResult::Err(ProverError::GuestError(output.status.to_string()));
    }

    Ok(parse_sgx_result(output.stdout)?)
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
    let proof = extract_field("proof");
    let quote = extract_field("quote");
    print_dirs();

    Ok(SgxResponse { proof, quote })
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

fn print_dirs() {
    println!("SGX output directories:");
    for dir in [
        GRAMINE_MANIFEST_TEMPLATE.get().unwrap(),
        PRIVATE_KEY.get().unwrap(),
    ] {
        println!(" {:?}", dir);
    }
}
