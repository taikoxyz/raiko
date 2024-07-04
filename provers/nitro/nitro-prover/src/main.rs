use nitro_prover::{protocol_helper::*, NitroProver, NON_HEX_PREFIX, PORT};
use raiko_lib::{input::GuestInput, prover::Prover};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;
use vsock::{VsockAddr, VsockListener};

#[tokio::main]
async fn main() {
    // start tracing + logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        println!(
            "Failed to set_global_default of tracing subscriber with details {}",
            e
        );
    }
    info!("Initializing");
    let listener = VsockListener::bind(&VsockAddr::new(libc::VMADDR_CID_ANY, PORT))
        .expect("bind and listen failed");
    info!("Listener socket binded. Starting main loop");
    for stream in listener.incoming() {
        info!("Received new proof request");
        match stream {
            Ok(mut v_stream) => {
                info!("Reading message from the socket");
                let Ok(data) = recv_message(&mut v_stream) else {
                    let msg = format!(
                        "{}Failed to read whole GuestInput bytes from socket!",
                        NON_HEX_PREFIX
                    );
                    info!(msg);
                    if let Err(e) = send_message(&mut v_stream, msg) {
                        error!("Failed to write error message back into socket. Client disconnected?, Details: {}", e);
                    }
                    continue;
                };
                let Ok(guest_input) = serde_json::from_str::<GuestInput>(&data) else {
                    let msg = format!(
                        "{}provided bytes are not json serialized guestinput",
                        NON_HEX_PREFIX
                    );
                    info!(msg);
                    if let Err(e) = send_message(&mut v_stream, msg) {
                        error!("Failed to write error message back into socket. Client disconnected?, Details: {}", e);
                    }
                    continue;
                };
                let block = guest_input.block.header.number;
                match NitroProver::run(guest_input, &Default::default(), &Default::default()).await
                {
                    Err(e) => {
                        let msg = format!(
                            "{}Failed to generate nitro proof for block {} with details {}",
                            NON_HEX_PREFIX, block, e
                        );
                        info!(msg);
                        if let Err(e) = send_message(&mut v_stream, msg) {
                            error!("Failed to write error message back into socket. Client disconnected?, Details: {}", e);
                        }
                        continue;
                    }
                    Ok(proof_value) => {
                        // safe unwrap - can't fail as is a `Value`
                        let proof = hex::encode(&serde_json::to_vec(&proof_value).unwrap());
                        let Ok(_) = send_message(&mut v_stream, &proof) else {
                            info!("Failed to write proof back into socket. Client disconnected?");
                            continue;
                        };
                        info!("Proved block {} with resulting proof {}", block, proof);
                    }
                }
            }
            // Nothing else we can do from enclave side...
            Err(err) => info!("Accept failed: {:?}", err),
        }
    }
}
