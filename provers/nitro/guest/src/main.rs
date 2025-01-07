use std::{
    thread,
    time::{Duration, SystemTime},
};

mod attest;
mod app_args;

use attest::get_pcr;
use log::{info, trace};

use anyhow::Error;
use attest::vsock_client::Client;

// vsock ports constants
// TODO: move to config file
const CMD_PORT: u32 = 5000;

fn main() -> Result<(), Error> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .init();
    info!("Setup logger done");
    info!("Setup client...");

    let client = Client::new(CMD_PORT)?;
    info!("Starting client...");
    client.run()?;

    loop {
        thread::sleep(Duration::from_secs(1));
        trace!("Main thread is alive {:?}", SystemTime::now());
    }
}
