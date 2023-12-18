use std::{str, path::PathBuf};

use tokio::process::Command;
use tracing::{debug, info};

use crate::prover::{
    consts::*,
    context::{Context, PowdrContext},
    request::{PowdrRequest, PowdrResponse},
    utils::{cache_file_path, guest_executable_path},
};


pub async fn execute_powdr(
    guest_path: &PathBuf, 
    cache_path: &PathBuf, 
    ctx: &PowdrContext, 
    req: &PowdrRequest
) -> Result<PowdrResponse, String> {
        unimplemented!()
}