extern crate rand;
extern crate secp256k1;

use anyhow::{anyhow, Result};
use clap::Parser;
use one_shot::{aggregate, shasta_aggregate};
use raiko_lib::input::{
    GuestBatchInput, GuestInput, RawAggregationGuestInput, ShastaRawAggregationGuestInput,
};

use crate::{
    app_args::{App, Command},
    one_shot::{bootstrap, load_bootstrap, one_shot, one_shot_batch},
};

mod app_args;
mod one_shot;
mod sgx_server;
mod signature;

#[tokio::main]
pub async fn main() -> Result<()> {
    let args = App::parse();

    match args.command {
        Command::OneShot(one_shot_args) => {
            println!("Starting one shot mode");
            let input: GuestInput =
                bincode::deserialize_from(std::io::stdin()).expect("unable to deserialize input");
            one_shot(args.global_opts, one_shot_args, input).await?;
        }
        Command::OneBatchShot(one_shot_args) => {
            println!("Starting one batch shot mode");
            let batch_input: GuestBatchInput = bincode::deserialize_from(std::io::stdin())
                .expect("unable to deserialize batch input");
            one_shot_batch(args.global_opts, one_shot_args, batch_input).await?;
        }
        Command::Aggregate(one_shot_args) => {
            println!("Starting one shot aggregate prove");
            let input: RawAggregationGuestInput =
                bincode::deserialize_from(std::io::stdin()).expect("unable to deserialize input");
            aggregate(args.global_opts, one_shot_args, input).await?;
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
        Command::Serve(server_args) => {
            println!("Sgx proof server");
            sgx_server::serve(server_args, args.global_opts).await;
        }
        Command::ShastaAggregate(one_shot_args) => {
            println!("Starting shasta aggregate prove");
            let input: ShastaRawAggregationGuestInput =
                bincode::deserialize_from(std::io::stdin()).expect("unable to deserialize input");
            shasta_aggregate(args.global_opts, one_shot_args, input).await?;
        }
    }

    Ok(())
}
