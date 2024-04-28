mod app_args;
mod setup_bootstrap;

use anyhow::Result;
use app_args::{App, Command};
use clap::Parser;
use setup_bootstrap::setup_bootstrap;

#[tokio::main]
pub async fn main() -> Result<()> {
    let args = App::parse();

    match args.command {
        Command::Bootstrap(sgx_bootstrap_args) => {
            println!("Setup bootstrapping: {:?}", sgx_bootstrap_args);
            setup_bootstrap(args.global_opts.secrets_dir, &sgx_bootstrap_args).await?;
        }
    }

    Ok(())
}
