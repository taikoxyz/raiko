#![feature(path_file_prefix)]

mod app_args;
mod signature;
mod one_shot;

extern crate rand;
extern crate secp256k1;

use anyhow::Result;
use app_args::{App, Command};
use clap::Parser;
use one_shot::one_shot;

#[tokio::main]
pub async fn main() -> Result<()> {
    let args = App::parse();

    match args.command {
        Command::OneShot(one_shot_args) => {
            println!("Starting one shot mode");
            one_shot(args.global_opts, one_shot_args).await?
        }
    }

    Ok(())
}
