#![cfg(feature = "foundry-verify")]

use once_cell::sync::Lazy;
use raiko_lib::prover::{ProverError, ProverResult};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tracing::{debug, info};

use crate::RaikoProofFixture;

static FIXTURE_PATH: Lazy<PathBuf> =
    Lazy::new(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../contracts/src/fixtures/"));
static CONTRACT_PATH: Lazy<PathBuf> =
    Lazy::new(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../contracts/src/exports/"));

pub static VERIFIER: Lazy<Result<PathBuf, ProverError>> = Lazy::new(init_verifier);

fn init_verifier() -> Result<PathBuf, ProverError> {
    // In cargo run, Cargo sets the working directory to the root of the workspace
    let contract_path = &*CONTRACT_PATH;
    info!("Contract dir: {contract_path:?}");
    let artifacts_dir = sp1_sdk::install::try_install_circuit_artifacts("plonk");
    // Create the destination directory if it doesn't exist
    fs::create_dir_all(contract_path)?;

    // Read the entries in the source directory
    for entry in fs::read_dir(artifacts_dir)? {
        let entry = entry?;
        let src = entry.path();

        // Check if the entry is a file and ends with .sol
        if src.is_file() && src.extension().map(|s| s == "sol").unwrap_or(false) {
            let out = contract_path.join(src.file_name().unwrap());
            fs::copy(&src, &out)?;
            println!("Copied: {:?}", src.file_name().unwrap());
        }
    }
    Ok(contract_path.clone())
}

/// verify the proof by using forge test, which involves downloading the whole sp1 sdk &
/// starting a forge environment to run test.
pub(crate) fn verify_sol_by_forge_test(fixture: &RaikoProofFixture) -> ProverResult<()> {
    assert!(VERIFIER.is_ok());
    debug!("===> Fixture: {fixture:#?}");

    // Save the fixture to a file.
    let fixture_path = &*FIXTURE_PATH;
    info!("Writing fixture to: {fixture_path:?}");

    if !fixture_path.exists() {
        std::fs::create_dir_all(fixture_path.clone())
            .map_err(|e| ProverError::GuestError(format!("Failed to create fixture path: {e}")))?;
    }
    std::fs::write(
        fixture_path.join("fixture.json"),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .map_err(|e| ProverError::GuestError(format!("Failed to write fixture: {e}")))?;

    let child = std::process::Command::new("forge")
        .arg("test")
        .current_dir(&*CONTRACT_PATH)
        .stdout(std::process::Stdio::inherit()) // Inherit the parent process' stdout
        .spawn();
    info!("Verification started {:?}", child);
    child.map_err(|e| ProverError::GuestError(format!("Failed to run forge: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_init_verifier() {
        VERIFIER.as_ref().expect("Failed to init verifier");
    }
}
