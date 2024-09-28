use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

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
    /// Bootstrap the application and then exit. The bootstrapping process generates the
    /// initial public-private key pair and stores it on the disk in an encrypted
    /// format using SGX encryption primitives.
    Bootstrap(BootstrapArgs),
}

#[derive(Debug, Args)]
pub struct BootstrapArgs {
    #[clap(long, default_value = "/etc/raiko/config.sgx.json")]
    /// Path to a config file that includes sufficient json args to request
    /// a proof of specified type. Curl json-rpc overrides its contents
    pub config_path: PathBuf,

    #[arg(long, default_value = "/etc/raiko/chain_spec_list.docker.json")]
    /// Path to a chain spec file that includes supported chain list
    pub chain_spec_path: PathBuf,

    #[arg(long, default_value = "holesky")]
    pub l1_network: String,

    #[arg(long, default_value = "taiko_a7")]
    pub network: String,

    /// block_num to get the verifier address for different fork
    #[arg(long, default_value = "0")]
    pub block_num: u64,
}

fn get_default_raiko_user_config_path(subdir: &str) -> PathBuf {
    let mut home_dir = dirs::home_dir().unwrap();
    home_dir.push(DEFAULT_RAIKO_USER_CONFIG_SUBDIR_PATH);
    home_dir.push(subdir);
    home_dir
}

#[derive(Debug, Args)]
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
