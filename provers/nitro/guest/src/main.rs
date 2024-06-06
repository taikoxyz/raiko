use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::GuestInput,
    protocol_instance::ProtocolInstance,
};
use std::{io, process};
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

fn main() -> anyhow::Result<()> {
    // start tracing + logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    // read and validate inputs
    info!("Starting Nitro guest and proof generation");
    let input: GuestInput = bincode::deserialize_from(io::stdin())?;
    if !input.taiko.skip_verify_blob {
        warn!("blob verification skip. terminating");
        process::exit(1);
    }
    // process the block
    let (header, _mpt_node) = TaikoStrategy::build_from(&input)?;
    // calculate the public input hash
    let pi = ProtocolInstance::new(&input, &header, raiko_lib::consts::VerifierType::Nitro)?;
    let pi_hash = pi.instance_hash();
    info!(
        "Block {}. PI data to be signed {}",
        input.block_number, pi_hash
    );
    // generate proof
    Ok(())
}
