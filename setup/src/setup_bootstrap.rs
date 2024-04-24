use std::{
    env,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
    str::FromStr,
};

use crate::app_args::BootstrapArgs;
use alloy_primitives::Address;
use anyhow::{Context, Result};
use serde_json::{Number, Value};
use sgx_prover::{bootstrap, check_bootstrap, register_sgx_instance, ELF_NAME};
use std::process::Command;
use tracing::info;

pub(crate) async fn setup_bootstrap(
    secret_dir: PathBuf,
    bootstrap_args: &BootstrapArgs,
) -> Result<()> {
    let cur_dir = env::current_exe()
        .expect("Fail to get current directory")
        .parent()
        .unwrap()
        .to_path_buf();

    let gramine_cmd = || -> Command {
        let mut cmd = Command::new("sudo");
        cmd.arg("gramine-sgx");
        cmd.current_dir(&cur_dir).arg(ELF_NAME);
        cmd
    };

    if let Err(_) = check_bootstrap(secret_dir.clone(), gramine_cmd()).await {
        let bootstrap_proof = bootstrap(secret_dir, gramine_cmd()).await?;

        let _register_res = register_sgx_instance(
            &bootstrap_proof.quote,
            &bootstrap_args.l1_rpc,
            bootstrap_args.l1_chain_id,
            Address::from_str(&bootstrap_args.sgx_verifier_address).unwrap(),
        )
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;

        //todo: update the config
        // Config file has the lowest preference
        let file = File::open(&bootstrap_args.config_path)?;
        let reader = BufReader::new(file);
        let mut file_config: Value = serde_json::from_reader(reader)?;
        file_config["sgx"]["instance_id"] = Value::Number(Number::from(_register_res));

        //save to the same file
        info!(
            "Saving bootstrap data file {}",
            bootstrap_args.config_path.display()
        );
        let json = serde_json::to_string_pretty(&file_config)?;
        fs::write(&bootstrap_args.config_path, json).context(format!(
            "Saving bootstrap data file {} failed",
            bootstrap_args.config_path.display()
        ))?;
    }

    Ok(())
}
