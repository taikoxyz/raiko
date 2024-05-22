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
    #[clap(long, default_value = "http://localhost:8545")]
    pub l1_rpc: String,
    #[clap(long, default_value = "31337")]
    pub l1_chain_id: u64,
    #[clap(long, default_value = "0x4826533B4897376654Bb4d4AD88B7faFD0C98528")]
    pub sgx_verifier_address: String,
    #[clap(long, default_value = "config.sgx.json")]
    /// Path to a config file that includes sufficient json args to request
    /// a proof of specified type. Curl json-rpc overrides its contents
    pub config_filename: String,
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
