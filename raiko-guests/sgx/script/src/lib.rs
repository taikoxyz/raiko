#![cfg(feature = "enable")]
use std::{fs::File, path::PathBuf, str};
use serde_json::Value;
use serde_with::serde_as;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, info};
use zeth_lib::input::{GuestInput, GuestOutput};

pub const SGX_ELF_PATH: &str = "todo"; // TODO

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SgxParam {
    pub instance_id: u64,

}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
}


pub async fn execute(input: GuestInput, output: GuestOutput, param: &SgxParam) -> Result<SgxResponse, String> {
    
    let mut file = File::create(format!("./target/debug/input.bin")).expect("unable to open file");
    bincode::serialize_into(&mut file, &input).expect("unable to serialize input");

    let sgx_elf_path = PathBuf::from(SGX_ELF_PATH.clone());
    let mut cmd = {
        let bin_directory = sgx_elf_path
            .parent()
            .clone()
            .expect("missing sgx executable directory");
        let bin = sgx_elf_path
            .file_name()
            .ok_or(String::from("missing sgx executable"))?;
        let mut cmd = Command::new("gramine-direct");
        cmd.current_dir(bin_directory)
            .arg(bin)
            .arg("one-shot");
        cmd
    };

    println!("sgx: {:?}", cmd);

    let default_sgx_instance_id: u32 = 0;
    let instance_id = param.instance_id;
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Could not run SGX guest application: {}", e.to_string()))?;
    info!("Sgx execution stderr: {:?}", str::from_utf8(&output.stderr));
    info!("Sgx execution stdout: {:?}", str::from_utf8(&output.stdout));
    if !output.status.success() {
        return Err(output.status.to_string());
    }

    println!("sgx done: {:?}", output.stdout);

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
