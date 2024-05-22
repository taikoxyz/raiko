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
use sgx_prover::{
    bootstrap, check_bootstrap, get_instance_id, register_sgx_instance, remove_instance_id,
    set_instance_id, ELF_NAME,
};
use std::process::Command;
use tracing::info;

pub(crate) async fn setup_bootstrap(
    secret_dir: PathBuf,
    config_dir: PathBuf,
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

    let mut instance_id = get_instance_id(&config_dir).ok();
    let need_init = check_bootstrap(secret_dir.clone(), gramine_cmd())
        .await
        .is_err()
        || instance_id.is_none();

    if need_init {
        let bootstrap_proof = bootstrap(secret_dir, gramine_cmd()).await?;
        // clean check file
        match remove_instance_id(&config_dir) {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }?;
        let register_id = register_sgx_instance(
            &bootstrap_proof.quote,
            &bootstrap_args.l1_rpc,
            bootstrap_args.l1_chain_id,
            Address::from_str(&bootstrap_args.sgx_verifier_address).unwrap(),
        )
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
        info!("Saving instance id {}", register_id,);
        // set check file
        set_instance_id(&config_dir, register_id)?;

        instance_id = Some(register_id);
    }
    // Always reset the configuration with a persistent instance ID upon restart.
    let config_path = config_dir.join(&bootstrap_args.config_filename);
    let file = File::open(&config_path)?;
    let reader = BufReader::new(file);
    let mut file_config: Value = serde_json::from_reader(reader)?;
    file_config["sgx"]["instance_id"] = Value::Number(Number::from(instance_id.unwrap()));

    //save to the same file
    info!("Saving bootstrap data file {}", config_path.display());
    let json = serde_json::to_string_pretty(&file_config)?;
    let new_config_path = config_path
        .with_extension("")
        .with_extension("new")
        .with_extension("json");
    fs::write(&new_config_path, json).context(format!(
        "Saving bootstrap data file {} failed",
        new_config_path.display()
    ))?;
    Ok(())
}
