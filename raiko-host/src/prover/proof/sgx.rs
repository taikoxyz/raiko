use std::path::Path;

use log::debug;
use tokio::process::Command;

use crate::prover::{
    consts::*,
    context::Context,
    request::{SgxRequest, SgxResponse},
    utils::{cache_file_path, guest_executable_path},
};

pub async fn execute_sgx(ctx: &Context, req: &SgxRequest) -> Result<SgxResponse, String> {
    let guest_path = guest_executable_path(&ctx.guest_path, SGX_PARENT_DIR);
    debug!("Guest path: {}", guest_path);
    let guest_path = Path::new(&guest_path);
    let mut cmd = {
        let bin_directory = guest_path
            .parent()
            .ok_or(String::from("missing sgx executable directory"))?;
        let mut cmd = Command::new("gramine-sgx");
        cmd.current_dir(bin_directory).arg(guest_path);
        cmd
    };
    let l1_cache_file = cache_file_path(&ctx.cache_path, req.block, true);
    let l2_cache_file = cache_file_path(&ctx.cache_path, req.block, false);
    let output = cmd
        .arg("--blocks_data_file")
        .arg(l2_cache_file)
        .arg("--l1_blocks_data_file")
        .arg(l1_cache_file)
        .arg("--prover")
        .arg(req.prover.to_string())
        .arg("--graffiti")
        .arg(&req.graffiti)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(output.status.to_string());
    }
    parse_sgx_result(ctx.sgx_context.instance_id, output.stdout)
}

fn parse_sgx_result(instance_id: u32, output: Vec<u8>) -> Result<SgxResponse, String> {
    // parse result of sgx execution
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;
    let mut instance_signature = String::new();
    let mut public_key = String::new();
    for line in output.lines() {
        if let Some(_instance_signature) = line.trim().strip_prefix(SGX_INSTANCE_SIGNATURE_PREFIX) {
            instance_signature = _instance_signature.trim().to_owned();
        }
        if let Some(_public_key) = line.trim().strip_prefix(SGX_PUBLIC_KEY_PREFIX) {
            public_key = _public_key.trim().to_owned();
        }
    }
    let mut proof = Vec::with_capacity(SGX_PROOF_LEN);
    proof.extend(instance_id.to_be_bytes());
    let public_key = hex::decode(public_key).map_err(|e| e.to_string())?;
    proof.extend(public_key);
    let instance_signature = hex::decode(instance_signature).map_err(|e| e.to_string())?;
    proof.extend(instance_signature);
    let proof = hex::encode(proof);
    Ok(SgxResponse { proof })
}
