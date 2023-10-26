use crate::prover::{
    constant::*,
    context::Context,
    request::{SgxRequest, SgxResponse},
    utils::{cache_file_path, guest_executable_path},
};
use std::path::Path;
use tokio::process::Command;

pub async fn execute_sgx(ctx: &Context, req: &SgxRequest) -> Result<SgxResponse, String> {
    let guest_path = guest_executable_path(&ctx.guest_path, SGX_PARENT_DIR);
    let guest_path = Path::new(&guest_path);
    let bin = guest_path
        .file_name()
        .ok_or(String::from("missing sgx executable bin"))?;
    let mut cmd = if req.no_sgx {
        let mut cmd = Command::new(bin);
        cmd.arg("--no-sgx");
        cmd
    } else {
        let bin_directory = guest_path
            .parent()
            .ok_or(String::from("missing sgx executable directory"))?;
        let mut cmd = Command::new("gramine-sgx");
        cmd.current_dir(bin_directory).arg(guest_path);
        cmd
    };
    let cache_file = cache_file_path(&ctx.cache_path, req.l2_block);
    let output = cmd
        .arg("--file")
        .arg(cache_file)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(output.status.to_string());
    }
    parse_sgx_result(output.stdout)
}

fn parse_sgx_result(output: Vec<u8>) -> Result<SgxResponse, String> {
    // parse result of sgx execution
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;
    let mut instance_signature = String::new();
    let mut public_key = String::new();
    let mut proof = String::new();
    let mut mr_enclave = String::new();
    for line in output.lines() {
        if let Some(_instance_signature) = line.trim().strip_prefix(SGX_INSTANCE_SIGNATURE_PREFIX) {
            instance_signature = _instance_signature.trim().to_owned();
        }
        if let Some(_public_key) = line.trim().strip_prefix(SGX_PUBLIC_KEY_PREFIX) {
            public_key = _public_key.trim().to_owned();
        }
        if let Some(_proof) = line.trim().strip_prefix(SGX_PROOF_PREFIX) {
            proof = _proof.trim().to_owned();
        }
    }
    Ok(SgxResponse {
        instance_signature,
        public_key,
        proof,
    })
}
