use std::str;
use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, info};

use crate::{
    metrics::inc_sgx_error,
    prover::{
        context::Context,
        request::{ProofRequest, SgxResponse},
        server::SGX_INSTANCE_ID,
    },
};
use zeth_lib::input::{GuestInput, GuestOutput};
use std::fs::{File, create_dir_all, copy};
use std::env;
use std::path::PathBuf;

pub const RAIKO_GUEST_EXECUTABLE: &str = "sgx-guest";
pub const INPUT_FILE: &str = "input.bin";
pub const CONFIG: &str = "../../raiko-guests/sgx/config/";

fn get_working_directory() -> PathBuf {
    let binding = env::current_exe().unwrap();
    binding.parent().unwrap().to_path_buf()
}

pub async fn execute_sgx(input: GuestInput, _output: GuestOutput, _ctx: &mut Context, req: &ProofRequest) -> Result<SgxResponse, String> {
    // Write the input to a file that will be read by the SGX insance
    let mut file = File::create(get_working_directory().join(INPUT_FILE)).expect("unable to open file");
    bincode::serialize_into(&mut file, &input).expect("unable to serialize input");

    // Working paths
    let working_directory = get_working_directory();
    let bin = RAIKO_GUEST_EXECUTABLE;

    // Support both SGX and the native backend for testing
    let no_sgx = true;
    let gramine = if no_sgx {
        "gramine-direct"
    } else {
        "gramine-sgx"
    };

    // TODO(Brecht): probably move some of this setup stuff to a build.rs file

    // Create required directories
    let directories = ["secrets", "config"];
    for dir in directories {
        create_dir_all(working_directory.join(dir)).unwrap();
    }

    // Sign the manifest
    let mut cmd = Command::new("gramine-manifest");
    cmd.current_dir(working_directory.clone())
        .arg("-Dlog_level=error")
        .arg("-Darch_libdir=/lib/x86_64-linux-gnu/")
        .arg(working_directory.join(CONFIG).join("raiko-guest.manifest.template"))
        .arg("sgx-guest.manifest")
        .output().await.map_err(|e| format!("Could not sign manfifest: {}", e.to_string()))?;

    // Copy dummy files
    if no_sgx {
        let files = ["attestation_type", "quote", "user_report_data"];
        for file in files {
            copy(working_directory.join(CONFIG).join("dummy_data").join(file), working_directory.join(file)).unwrap();
        }
    }

    // Bootstrap
    let mut cmd = Command::new(gramine);
    cmd.current_dir(working_directory.clone()).arg(bin).arg("bootstrap").output().await.map_err(|e| format!("Could not run SGX guest boostrap: {}", e.to_string()))?;

    // Prove
    let mut cmd = Command::new(gramine);
    let cmd = cmd.current_dir(working_directory).arg(bin).arg("one-shot");
    let default_sgx_instance_id: u32 = 0;
    let instance_id = SGX_INSTANCE_ID.get().unwrap_or(&default_sgx_instance_id);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Could not run SGX guest prover: {}", e.to_string()))?;
    print!("Sgx execution stderr: {}", str::from_utf8(&output.stderr).unwrap());
    print!("Sgx execution stdout: {}", str::from_utf8(&output.stdout).unwrap());

    if !output.status.success() {
        inc_sgx_error(req.block_number);
        return Err(output.status.to_string());
    }

    parse_sgx_result(output.stdout)
}

fn parse_sgx_result(output: Vec<u8>) -> Result<SgxResponse, String> {
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

    Ok(SgxResponse { proof, quote })
}
