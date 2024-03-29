use std::{
    env,
    fs::{copy, create_dir_all, remove_file, File},
    path::{Path, PathBuf},
    str,
};

use raiko_lib::input::{GuestInput, GuestOutput};
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

pub const RAIKO_GUEST_EXECUTABLE: &str = "sgx-guest";
pub const INPUT_FILE: &str = "input.bin";
pub const CONFIG: &str = "../../raiko-guests/sgx/config/";

fn get_working_directory() -> PathBuf {
    let binding = env::current_exe().unwrap();
    binding.parent().unwrap().to_path_buf()
}

pub async fn execute_sgx(
    input: GuestInput,
    _output: GuestOutput,
    _ctx: &mut Context,
    req: &ProofRequest,
) -> Result<SgxResponse, String> {
    // Write the input to a file that will be read by the SGX instance
    let mut file =
        File::create(get_working_directory().join(INPUT_FILE)).expect("unable to open file");
    bincode::serialize_into(&mut file, &input).expect("unable to serialize input");

    // Working paths
    let working_directory = get_working_directory();
    let bin = RAIKO_GUEST_EXECUTABLE;

    // Support both SGX and the direct backend for testing
    let direct_mode = match env::var("SGX_DIRECT") {
        Ok(value) => value == "1",
        Err(_) => false,
    };

    // Print a warning when running in direct mode
    if direct_mode {
        println!("WARNING: running SGX in direct mode!");
    }

    // TODO(Brecht): probably move some of this setup stuff to a build.rs file

    // Create required directories
    let directories = ["secrets", "config"];
    for dir in directories {
        create_dir_all(working_directory.join(dir)).unwrap();
    }

    // Generate the manifest
    let mut cmd = Command::new("gramine-manifest");
    let output = cmd
        .current_dir(working_directory.clone())
        .arg("-Dlog_level=error")
        .arg("-Darch_libdir=/lib/x86_64-linux-gnu/")
        .arg(format!(
            "-Ddirect_mode={}",
            if direct_mode { "1" } else { "0" }
        ))
        .arg(
            working_directory
                .join(CONFIG)
                .join("raiko-guest.manifest.template"),
        )
        .arg("sgx-guest.manifest")
        .output()
        .await
        .map_err(|e| format!("Could not generate manfifest: {}", e.to_string()))?;
    print!(
        "Sgx manifest stderr: {}\n",
        str::from_utf8(&output.stderr).unwrap()
    );
    print!(
        "Sgx manifest stdout: {}\n",
        str::from_utf8(&output.stdout).unwrap()
    );

    if direct_mode {
        // Copy dummy files
        let files = ["attestation_type", "quote", "user_report_data"];
        for file in files {
            copy(
                working_directory.join(CONFIG).join("dummy_data").join(file),
                working_directory.join(file),
            )
            .unwrap();
        }
    } else {
        // Generate a private key
        let mut cmd = Command::new("gramine-sgx-gen-private-key");
        cmd.current_dir(working_directory.clone())
            .arg("-f")
            .output()
            .await
            .map_err(|e| format!("Could not generate SGX private key: {}", e.to_string()))?;

        // Sign the manifest
        let mut cmd = Command::new("gramine-sgx-sign");
        cmd.current_dir(working_directory.clone())
            .arg("--manifest")
            .arg("sgx-guest.manifest")
            .arg("--output")
            .arg("sgx-guest.manifest.sgx")
            .output()
            .await
            .map_err(|e| format!("Could not sign manfifest: {}", e.to_string()))?;
    }

    let gramine_cmd = || -> Command {
        if direct_mode {
            Command::new("gramine-direct")
        } else {
            let mut cmd = Command::new("sudo");
            cmd.arg("gramine-sgx");
            cmd
        }
    };

    // Bootstrap
    // First delete the private key if it already exists
    let private_key_path = working_directory.join("secrets/priv.key");
    if private_key_path.exists() {
        if let Err(e) = remove_file(private_key_path) {
            println!("Error deleting file: {}", e);
        }
    }
    // Generate a new one
    let mut cmd = gramine_cmd();
    let output = cmd
        .current_dir(working_directory.clone())
        .arg(bin)
        .arg("bootstrap")
        .output()
        .await
        .map_err(|e| format!("Could not run SGX guest boostrap: {}", e.to_string()))?;
    print!(
        "Sgx bootstrap stderr: {}\n",
        str::from_utf8(&output.stderr).unwrap()
    );
    print!(
        "Sgx bootstrap stdout: {}\n",
        str::from_utf8(&output.stdout).unwrap()
    );

    // Prove
    let mut cmd = gramine_cmd();
    let cmd = cmd.current_dir(working_directory).arg(bin).arg("one-shot");
    let default_sgx_instance_id: u32 = 0;
    let instance_id = SGX_INSTANCE_ID.get().unwrap_or(&default_sgx_instance_id);
    let output = cmd
        .arg("--sgx-instance-id")
        .arg(instance_id.to_string())
        .output()
        .await
        .map_err(|e| format!("Could not run SGX guest prover: {}", e.to_string()))?;
    print!(
        "Sgx execution stderr: {}\n",
        str::from_utf8(&output.stderr).unwrap()
    );
    print!(
        "Sgx execution stdout: {}\n",
        str::from_utf8(&output.stdout).unwrap()
    );

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
