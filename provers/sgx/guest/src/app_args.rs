use std::{collections::BTreeMap, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use raiko_lib::consts::SpecId;

const DEFAULT_RAIKO_USER_CONFIG_SUBDIR_PATH: &str = ".config/raiko";

#[derive(Debug, Parser)]
pub struct App {
    #[clap(flatten)]
    pub global_opts: GlobalOpts,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Prove (i.e. sign) a single block and exit.
    OneShot(OneShotArgs),
    /// Prove (i.e. sign) a single block and exit.
    OneBatchShot(OneShotArgs),
    /// Aggregate proofs
    Aggregate(OneShotArgs),
    /// Bootstrap the application and then exit. The bootstrapping process generates the
    /// initial public-private key pair and stores it on the disk in an encrypted
    /// format using SGX encryption primitives.
    Bootstrap,
    /// Check if bootstrap is readable
    Check,
    /// Sgx server to process incoming
    Serve(ServerArgs),
    /// Shasta aggregate proofs
    ShastaAggregate(OneShotArgs),
}

#[derive(Debug, Args)]
pub struct OneShotArgs {
    #[clap(long)]
    pub sgx_instance_id: u32,
}

#[derive(Debug, Args, Clone)]
pub struct ServerArgs {
    #[clap(long, value_parser = parse_specid_map)]
    pub sgx_instance_ids: BTreeMap<SpecId, u32>,
    #[clap(long)]
    pub address: String,
    #[clap(long)]
    pub port: u32,
}

fn parse_specid_map(s: &str) -> Result<BTreeMap<SpecId, u32>, String> {
    serde_json::from_str(s).map_err(|e| e.to_string())
}

fn get_default_raiko_user_config_path(subdir: &str) -> PathBuf {
    let mut home_dir = dirs::home_dir().unwrap();
    home_dir.push(DEFAULT_RAIKO_USER_CONFIG_SUBDIR_PATH);
    home_dir.push(subdir);
    home_dir
}

#[derive(Debug, Args, Clone)]
pub struct GlobalOpts {
    #[clap(short, long, default_value=get_default_raiko_user_config_path("secrets").into_os_string())]
    /// Path to the directory with the encrypted private keys being used to sign the
    /// blocks. For more details on the encryption see:
    /// https://gramine.readthedocs.io/en/stable/manifest-syntax.html#encrypted-files
    pub secrets_dir: PathBuf,

    #[clap(short, long, default_value=get_default_raiko_user_config_path("config").into_os_string())]
    /// Path to the directory containing Raiko configuration files.
    pub config_dir: PathBuf,
}
