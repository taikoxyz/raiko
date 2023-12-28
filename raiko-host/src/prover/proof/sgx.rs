use std::{process::Command, str};

use tracing::{debug, info};

use crate::prover::{
    consts::*,
    context::Context,
    request::{SgxRequest, SgxResponse},
    utils::guest_executable_path,
};

use crate::metrics::inc_sgx_error;

pub fn execute_sgx(ctx: Context, req: SgxRequest) -> Result<SgxResponse, String> {
    let guest_path = guest_executable_path(&ctx.guest_path, SGX_PARENT_DIR);
    debug!("Guest path: {:?}", guest_path);
    let mut cmd = {
        let bin_directory = guest_path
            .parent()
            .ok_or(String::from("missing sgx executable directory"))?;
        let bin = guest_path
            .file_name()
            .ok_or(String::from("missing sgx executable"))?;
        let mut cmd = Command::new("sudo");
        cmd.current_dir(bin_directory)
            .arg("gramine-sgx")
            .arg(bin)
            .arg("--log-path")
            .arg(ctx.log_path.unwrap_or(DEFAULT_SGX_LOG_PATH.into()))
            .arg("one-shot");
        cmd
    };
    let l1_rpc = req.l1_rpc.clone();
    let l2_rpc = req.l2_rpc.clone();
    let output = cmd
        .arg("--l1-rpc")
        .arg(l1_rpc)
        .arg("--l2-rpc")
        .arg(l2_rpc)
        .arg("--prover")
        .arg(req.prover.to_string())
        .arg("--graffiti")
        .arg(req.graffiti.to_string())
        .arg("--sgx-instance-id")
        .arg(ctx.sgx_context.instance_id.to_string())
        .arg("--block")
        .arg(req.block.to_string())
        .output()
        .map_err(|e| e.to_string())?;

    info!("Sgx execution stderr: {:?}", str::from_utf8(&output.stderr));
    info!("Sgx execution stdout: {:?}", str::from_utf8(&output.stdout));
    if !output.status.success() {
        inc_sgx_error(req.block);
        return Err(output.status.to_string());
    }
    parse_sgx_result(output.stdout)
}

fn parse_sgx_result(output: Vec<u8>) -> Result<SgxResponse, String> {
    // parse result of sgx execution
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;
    let mut proof = String::new();
    for line in output.lines() {
        if let Some(_proof) = line.trim().strip_prefix(SGX_PROOF_PREFIX) {
            proof = _proof.trim().to_owned();
        }
    }
    Ok(SgxResponse { proof })
}
