use std::{
    env,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
    process::Command,
};

use anyhow::{anyhow, Context, Result};
use raiko_lib::consts::{SupportedChainSpecs, VerifierType};
use serde_json::{Number, Value};
use sgx_prover::{
    bootstrap, check_bootstrap, get_instance_id, register_sgx_instance, remove_instance_id,
    set_instance_id, ELF_NAME,
};

use crate::app_args::BootstrapArgs;

pub(crate) async fn setup_bootstrap(
    secret_dir: PathBuf,
    config_dir: PathBuf,
    bootstrap_args: &BootstrapArgs,
) -> Result<()> {
    let chain_specs = SupportedChainSpecs::merge_from_file(bootstrap_args.chain_spec_path.clone())?;
    let l1_chain_spec = chain_specs
        .get_chain_spec(&bootstrap_args.l1_network)
        .ok_or_else(|| anyhow!("Unsupported l1 network: {}", bootstrap_args.l1_network))?;

    let taiko_chain_spec = chain_specs
        .get_chain_spec(&bootstrap_args.network)
        .ok_or_else(|| anyhow!("Unsupported l2 network: {}", bootstrap_args.l1_network))?;

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

    let mut instance_id = get_instance_id(&config_dir)?;
    let need_init = check_bootstrap(secret_dir.clone(), gramine_cmd())
        .await
        .map_err(|e| {
            println!("Error checking bootstrap: {e:?}");
            e
        })
        .is_err()
        || instance_id.is_none();

    println!("Instance ID: {instance_id:?}");

    if need_init {
        // clean check file
        remove_instance_id(&config_dir)?;
        let bootstrap_proof = bootstrap(secret_dir, gramine_cmd()).await?;
        let verifier_address = taiko_chain_spec.get_fork_verifier_address(
            bootstrap_args.block_num,
            VerifierType::SGX,
        )?;
        let register_id = register_sgx_instance(
            &bootstrap_proof.quote,
            &l1_chain_spec.rpc,
            l1_chain_spec.chain_id,
            verifier_address,
        )
        .await
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
        println!("Saving instance id {register_id}");
        // set check file
        set_instance_id(&config_dir, register_id)?;

        instance_id = Some(register_id);
    }
    // Always reset the configuration with a persistent instance ID upon restart.
    let file = File::open(&bootstrap_args.config_path)?;
    let reader = BufReader::new(file);
    let mut file_config: Value = serde_json::from_reader(reader)?;
    file_config["sgx"]["instance_id"] = Value::Number(Number::from(instance_id.unwrap()));

    //save to the same file
    let new_config_path = config_dir.join("config.sgx.json");
    println!("Saving bootstrap data file {}", new_config_path.display());
    let json = serde_json::to_string_pretty(&file_config)?;
    fs::write(&new_config_path, json).context(format!(
        "Saving bootstrap data file {} failed",
        new_config_path.display()
    ))?;
    Ok(())
}
