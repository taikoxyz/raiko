use nitro_prover::protocol_helper::*;
use nitro_prover::NitroProver;
use nix::sys::socket::listen as listen_vsock;
use nix::sys::socket::Backlog;
use nix::sys::socket::{accept, bind, socket};
use nix::sys::socket::{AddressFamily, SockFlag, SockType, VsockAddr};
use raiko_lib::{input::GuestInput, prover::Prover};
use std::os::fd::AsRawFd;

const VMADDR_CID_ANY: u32 = 0xFFFFFFFF;
const BUF_MAX_LEN: usize = 8192;
const PORT: u32 = 26000;
// Maximum number of outstanding connections in the socket's
// listen queue
const BACKLOG: i32 = 128;

#[tokio::main]
async fn main() {
    let socket_fd = socket(
        AddressFamily::Vsock,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )
    .map_err(|err| format!("Create socket failed: {:?}", err))
    .unwrap(); // intentional panic

    let sockaddr = VsockAddr::new(VMADDR_CID_ANY, PORT);

    bind(socket_fd.as_raw_fd(), &sockaddr)
        .map_err(|err| format!("Bind failed: {:?}", err.to_string()))
        .unwrap(); // intentional panic

    listen_vsock(&socket_fd, Backlog::new(BACKLOG).unwrap()) // intentional panic
        .map_err(|err| format!("Listen failed: {:?}", err.to_string()))
        .unwrap(); // intentional panic

    loop {
        match accept(socket_fd.as_raw_fd()) {
            Ok(fd) => {
                let Ok(len) = recv_u64(fd) else {
                    println!("Failed to read length of payload from socket!");
                    continue;
                };
                let mut buf = [0u8; BUF_MAX_LEN];
                let Ok(()) = recv_loop(fd, &mut buf, len) else {
                    println!("Failed to read whole GuestInput bytes from socket!");
                    continue;
                };
                let Ok(guest_input) = serde_json::from_slice::<GuestInput>(&buf) else {
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
                let Ok(_) = send_loop(fd, &proof, proof.len() as u64) else {
                    println!("Failed to write proof back into socket. Client disconnected?");
                    continue;
                };
            }
            Err(err) => println!("Accept failed: {:?}", err),
        }
    }
}
