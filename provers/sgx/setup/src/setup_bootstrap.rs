use alloy_primitives::Address;
use anyhow::{anyhow, Context, Result};
use file_lock::{FileLock, FileOptions};
use raiko_lib::{
    consts::{ChainSpec, SpecId, SupportedChainSpecs},
    proof_type::ProofType,
};
use serde_json::Value;
use sgx_prover::{
    bootstrap, check_bootstrap, get_instance_id, register_sgx_instance, remove_instance_id,
    set_instance_id, ForkRegisterId, ELF_NAME,
};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
    process::Command,
};
use tracing::warn;

use crate::app_args::BootstrapArgs;

pub(crate) async fn setup_bootstrap(
    secret_dir: PathBuf,
    config_dir: PathBuf,
    bootstrap_args: &BootstrapArgs,
) -> Result<()> {
    // Lock the bootstrap process to prevent multiple instances from running concurrently.
    // Block until the lock is acquired.
    // Create the lock file if it does not exist.
    // Drop the lock file when the lock goes out of scope by drop guard.
    let _filelock = FileLock::lock(
        config_dir.join("bootstrap.lock"),
        true,
        FileOptions::new().create(true).write(true),
    )?;
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

    let fork_verifier_pairs = get_hard_fork_verifiers(&taiko_chain_spec);
    let mut registered_fork_ids = get_instance_id(&config_dir)?;
    let need_init = check_bootstrap(secret_dir.clone(), gramine_cmd())
        .await
        .map_err(|e| {
            println!("Error checking bootstrap: {e:?}");
            e
        })
        .is_err()
        || registered_fork_ids.is_none()
        || fork_verifier_pairs
            .clone()
            .into_values()
            .flat_map(|v| v)
            .any(|id| !registered_fork_ids.clone().unwrap().contains_key(&id));

    println!("Instance ID: {registered_fork_ids:?}");

    if need_init {
        // clean check file
        remove_instance_id(&config_dir)?;
        let bootstrap_proof = bootstrap(secret_dir, gramine_cmd()).await?;
        let mut fork_register_id: ForkRegisterId = BTreeMap::new();
        for (verifier_addr, spec_ids) in fork_verifier_pairs.iter() {
            let register_id = register_sgx_instance(
                &bootstrap_proof.quote,
                &l1_chain_spec.rpc,
                l1_chain_spec.chain_id,
                *verifier_addr,
            )
            .await
            .map_err(|e| anyhow::Error::msg(e.to_string()))?;
            for spec_id in spec_ids {
                fork_register_id.insert(*spec_id, register_id);
            }
        }
        println!("Saving instance id {registered_fork_ids:?}");
        // set check file
        set_instance_id(&config_dir, &fork_register_id)?;
        registered_fork_ids = Some(fork_register_id);
    }
    // Always reset the configuration with a persistent instance ID upon restart.
    let file = File::open(&bootstrap_args.config_path)?;
    let reader = BufReader::new(file);
    let mut file_config: Value = serde_json::from_reader(reader)?;
    let sgx_instance_json_value = serde_json::to_value(registered_fork_ids)?;
    file_config["sgx"]["instance_ids"] = sgx_instance_json_value;

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

fn get_hard_fork_verifiers(taiko_chain_spec: &ChainSpec) -> BTreeMap<Address, Vec<SpecId>> {
    let mut fork_verifiers: BTreeMap<Address, Vec<SpecId>> =
        BTreeMap::<Address, Vec<SpecId>>::new();
    taiko_chain_spec
        .verifier_address_forks
        .iter()
        .for_each(
            |(spec_id, verifiers)| match verifiers.get(&ProofType::Sgx) {
                Some(verifer_addr) => match verifer_addr {
                    Some(addr) => {
                        fork_verifiers
                            .entry(addr.clone())
                            .or_insert_with(Vec::new)
                            .push(*spec_id);
                    }
                    None => warn!("No verifier for fork {spec_id:?}"),
                },
                None => warn!("No verifier for fork {spec_id:?}"),
            },
        );
    fork_verifiers
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use super::*;
    use env_logger;
    use raiko_lib::consts::Network;
    use tracing::info;
    use tracing::log::LevelFilter;

    #[test]
    fn test_hard_fork_verifier() {
        env_logger::Builder::new()
            .filter_level(LevelFilter::Trace)
            .init();
        let taiko_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&Network::TaikoMainnet.to_string())
            .unwrap();
        let fork_verifier_pairs = get_hard_fork_verifiers(&taiko_chain_spec);
        info!("fork_verifier_pairs = {fork_verifier_pairs:?}")
    }

    #[test]
    fn test_save_config_file() {
        let registered_fork_ids: ForkRegisterId =
            serde_json::from_str("{\"HEKLA\": 1, \"ONTAKE\": 2}").expect("serde json ok");
        let file = File::open("/tmp/test.config.json").expect("open tmp config file");
        let reader = BufReader::new(file);
        let mut file_config: Value = serde_json::from_reader(reader).expect("read file");
        println!("in file_config: {file_config}");
        let sgx_instance_json_value =
            serde_json::to_value(registered_fork_ids.clone()).expect("btree to value");
        file_config["sgx"]["instance_ids"] = sgx_instance_json_value;
        println!("updated file_config: {file_config}");
        set_instance_id(Path::new("/tmp"), &registered_fork_ids).expect("save register ids")
    }

    #[test]
    fn test_reload_config_file_need_init() {
        let file = File::open("/tmp/registered.json").expect("open tmp config file");
        let reader = BufReader::new(file);
        let registered_fork_ids: ForkRegisterId =
            serde_json::from_reader(reader).expect("read file");
        println!("in file_config: {registered_fork_ids:?}");
        let taiko_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&Network::TaikoMainnet.to_string())
            .unwrap();
        let fork_verifier_pairs = get_hard_fork_verifiers(&taiko_chain_spec);
        let need_init = fork_verifier_pairs
            .clone()
            .into_values()
            .flat_map(|v| v)
            .any(|id| !registered_fork_ids.clone().contains_key(&id));
        assert!(
            need_init,
            "{fork_verifier_pairs:?} is different than {registered_fork_ids:?}"
        )
    }
}
