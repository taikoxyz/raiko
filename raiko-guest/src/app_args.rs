use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};
use zeth_primitives::{Address, B256};

#[derive(Debug, Parser)]
pub struct App {
    #[clap(flatten)]
    pub global_opts: GlobalOpts,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Server(ServerArgs),
    OneShot(OneShotArgs),
    /// Bootstrap the application and exit. Bootstraping process creates the first
    /// public-private key pair and saves it on disk in encrypted form.
    Bootstrap,
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[clap(short, long, require_equals = true, default_value = "127.0.0.1:8080")]
    pub addr: String,
}

#[derive(Debug, Args)]
pub struct OneShotArgs {
    #[clap(long)]
    pub l1_blocks_data_file: Option<PathBuf>,
    #[clap(long)]
    /// Path of the *.json.gz file with the block data.
    pub l2_blocks_data_file: Option<PathBuf>,
    #[clap(long)]
    pub l1_rpc: Option<String>,
    #[clap(long)]
    pub l2_rpc: Option<String>,
    #[clap(long)]
    pub block: u64,
    #[clap(long)]
    pub prover: Address,
    #[clap(long)]
    pub graffiti: B256,
    #[clap(long)]
    pub sgx_instance_id: u32,
}

#[derive(Debug, Args)]
pub struct GlobalOpts {
    #[clap(short, long, require_equals = true, default_value = "/secrets")]
    /// Path to the directory with the encrypted private keys being used to sign the
    /// blocks.
    pub secrets_dir: PathBuf,

    #[clap(long, short, global = true, action = ArgAction::Count)]
    /// Verbosity of the application. Use multiple times to increase verbosity.
    pub verbose: u8,

    #[clap(long)]
    pub log_path: Option<PathBuf>,

    #[clap(long, default_value = "7")]
    pub max_log_days: Option<usize>,
}
