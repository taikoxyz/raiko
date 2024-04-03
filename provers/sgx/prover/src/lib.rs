#![cfg(feature = "enable")]
use std::{
    env,
    fs::{self, copy, create_dir_all, remove_file, File},
    path::{Path, PathBuf},
    process::Output,
    str,
};

use alloy_sol_types::SolValue;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{Prover, ProverError, ProverResult},
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
    pub input_path: Option<PathBuf>,
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

pub struct SgxProver;

impl Prover for SgxProver {
    type ProofParam = SgxParam;
    type ProofResponse = SgxResponse;

    async fn run(
        input: GuestInput,
        _output: GuestOutput,
        param: Self::ProofParam,
    ) -> ProverResult<Self::ProofResponse> {
        // Support both SGX and the direct backend for testing. For SGX, we assume that we are
        // running in a Docker container.
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        println!(
            "WARNING: running SGX in {} mode!",
            if direct_mode { "direct" } else { "hardware" }
        );

        // Prepare prerequisites if running in direct mode. For SGX mode, we assume they are
        // already prepared by the Docker image.
        let cur_dir = if direct_mode {
            let cur_dir = env::current_exe()
                .expect("Fail to get current directory")
                .parent()
                .unwrap()
                .to_path_buf();
            println!("Current directory: {:?}\n", cur_dir);
            prepare_working_directory(cur_dir.clone()).await?;
            cur_dir
        } else {
            PathBuf::from("/opt/raiko/provers/sgx")
        };

        // If a cached input file is not provided, write the input to a file that will be read
        // by the SGX instance. All input files should be located in /tmp/sgx, as specified in
        // Gramine's manifest file.
        let input_file = match param.input_path {
            Some(path) => {
                let destination = "/tmp/sgx/";
                let destination_path = Path::new(destination).join(path.file_name().unwrap());
                fs::copy(&path, &destination_path).expect("Failed to copy input file");
                destination_path
            }
            None => {
                let path = Path::new("/tmp/sgx/").join(INPUT_FILE_NAME);
                bincode::serialize_into(File::create(&path).expect("Unable to open file"), &input)
                    .expect("Unable to serialize input");
                path
            }
        };

        // Form the relevant Gramine command prefix
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

        // Generate a new private key if in direct mode. In hardware mode, we assume it has
        // already been generated.
        if direct_mode {
            let output = gramine_cmd()
                .arg("bootstrap")
                .output()
                .await
                .map_err(|e| format!("Could not run SGX guest boostrap: {}", e))?;
            print_output(&output, "Sgx bootstrap");
        }

        // Prove
        let output = gramine_cmd()
            .arg("one-shot")
            .arg("--sgx-instance-id")
            .arg(param.instance_id.to_string())
            .arg("--blocks-data-file")
            .arg(input_file)
            .output()
            .await
            .map_err(|e| format!("Could not run SGX guest prover: {}", e))?;

        print_output(&output, "Sgx execution");

        if !output.status.success() {
            // inc_sgx_error(req.block_number);
            return ProverResult::Err(ProverError::GuestError(output.status.to_string()));
        }

        Ok(parse_sgx_result(output.stdout)?)
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
// (direct) mode. It is not applicable in hardware mode.
async fn prepare_working_directory(cur_dir: PathBuf) -> ProverResult<()> {
    // Create required directories
    let directories = ["secrets", "config"];
    for dir in directories {
        create_dir_all(cur_dir.join(dir)).unwrap();
    }
    let gramine_manifest_template = cur_dir.join(CONFIG).join("raiko-guest.manifest.template");

    // Bootstrap. First delete the private key if it already exists.
    let path = cur_dir.join("secrets").join("priv.key");
    if path.exists() {
        if let Err(e) = remove_file(&path) {
            println!("Error deleting file: {}", e);
        }
    }

    // Copy dummy files in direct mode
    let files = ["attestation_type", "quote", "user_report_data"];
    for file in files {
        copy(
            cur_dir.join(CONFIG).join("dummy_data").join(file),
            cur_dir.join(file),
        )
        .unwrap();
    }

    // Generate Gramine's manifest
    let mut cmd = Command::new("gramine-manifest");
    let output = cmd
        .current_dir(cur_dir.clone())
        .arg("-Dlog_level=error")
        .arg("-Darch_libdir=/lib/x86_64-linux-gnu/")
        .arg("-Ddirect_mode=1")
        .arg(gramine_manifest_template)
        .arg("sgx-guest.manifest")
        .output()
        .await
        .map_err(|e| format!("Could not generate manfifest: {}", e))?;

    print_output(&output, "Generate manifest");

    Ok(())
}

fn parse_sgx_result(output: Vec<u8>) -> ProverResult<SgxResponse, String> {
    let mut json_value: Option<Value> = None;
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;

    // Assume that the first line which is valid JSON is the one we need to parse
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

fn print_output(output: &Output, name: &str) {
    for (output, value) in &[("stderr", &output.stderr), ("stdout", &output.stdout)] {
        println!("{} {}: {}", name, output, str::from_utf8(value).unwrap());
    }
}
