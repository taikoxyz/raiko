extern crate rand;
extern crate secp256k1;

use crate::{
    app_args::{App, Command},
    one_shot::{bootstrap, load_bootstrap, one_shot},
};
use anyhow::{anyhow, Result};
use clap::Parser;

mod app_args;
mod one_shot;

#[tokio::main]
pub async fn main() -> Result<()> {
    let args = App::parse();

    match args.command {
        Command::OneShot(one_shot_args) => {
            println!("Starting one shot mode");
            one_shot(args.global_opts, one_shot_args).await?
        }
        Command::Bootstrap => {
            println!("Bootstrapping the app");
            bootstrap(args.global_opts)?
        }
        Command::Check => {
            println!("Checking if bootstrap is readable");
            load_bootstrap(&args.global_opts.secrets_dir)
                .map_err(|err| anyhow!("check booststrap failed: {err}"))?;
        }
    }

    Ok(())
}
