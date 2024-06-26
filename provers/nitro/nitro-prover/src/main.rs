use nitro_prover::{protocol_helper::*, NitroProver, BUF_MAX_LEN, PORT};
// use nix::sys::socket::listen as listen_vsock;
// use nix::sys::socket::Backlog;
// use nix::sys::socket::{accept, bind, socket};
// use nix::sys::socket::{AddressFamily, SockFlag, SockType, VsockAddr};
use raiko_lib::{input::GuestInput, prover::Prover};
use std::os::fd::AsRawFd;
use vsock::{VsockAddr, VsockListener, VsockStream};

// Maximum number of outstanding connections in the socket's
// listen queue
const BACKLOG: i32 = 128;

#[tokio::main]
async fn main() {
    // let socket_fd = socket(
    //     AddressFamily::Vsock,
    //     SockType::Stream,
    //     SockFlag::empty(),
    //     None,
    // )
    // .map_err(|err| format!("Create socket failed: {:?}", err))
    // .unwrap(); // intentional panic

    // let sockaddr = VsockAddr::new(VMADDR_CID_ANY, PORT);

    // bind(socket_fd.as_raw_fd(), &sockaddr)
    //     .map_err(|err| format!("Bind failed: {:?}", err.to_string()))
    //     .unwrap(); // intentional panic

    // listen_vsock(&socket_fd, Backlog::new(BACKLOG).unwrap()) // intentional panic
    //     .map_err(|err| format!("Listen failed: {:?}", err.to_string()))
    //     .unwrap(); // intentional panic
    println!("Initializing");
    let listener = VsockListener::bind(&VsockAddr::new(libc::VMADDR_CID_ANY, PORT))
        .expect("bind and listen failed");

    for stream in listener.incoming() {
        match stream {
            Ok(mut v_stream) => {
                let Ok(data) = recv_message(&mut v_stream) else {
                    println!("Failed to read whole GuestInput bytes from socket!");
                    continue;
                };
                let Ok(guest_input) = serde_json::from_slice::<GuestInput>(&data) else {
                    println!("Provided bytes are not json serialized GuestInput");
                    continue;
                };
                let block = guest_input.block_number;
                let Ok(proof_value) =
                    NitroProver::run(guest_input, &Default::default(), &Default::default()).await
                else {
                    println!("Failed to generate nitro proof for block {}", block);
                    continue;
                };

                let Ok(proof) = serde_json::from_value::<Vec<u8>>(proof_value) else {
                    println!("Proof type unexpected for block {}!", block);
                    continue;
                };
                let Ok(_) = send_message(&mut v_stream, hex::encode(proof)) else {
                    println!("Failed to write proof back into socket. Client disconnected?");
                    continue;
                };
            }
            Err(err) => println!("Accept failed: {:?}", err),
        }
    }
}
