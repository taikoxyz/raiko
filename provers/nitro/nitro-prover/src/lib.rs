use crate::protocol_helper::*;
use aws_nitro_enclaves_nsm_api::{
    api::{Request, Response},
    driver::{nsm_exit, nsm_init, nsm_process_request},
};
// use nix::{
//     sys::socket::{
//         connect, shutdown, socket, AddressFamily, Shutdown, SockFlag, SockType, VsockAddr,
//     },
//     unistd::close,
// };
use protocol_helper::*;
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{Proof, Prover, ProverConfig, ProverError, ProverResult},
    signature::{generate_key, sign_message},
};
use serde_bytes::ByteBuf;
use std::os::unix::io::{AsRawFd, RawFd};
use std::process;
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;
use vsock::{VsockAddr, VsockStream};

pub mod protocol_helper;

pub const CID: u32 = 16;
pub const PORT: u32 = 26000;
pub const BUF_MAX_LEN: usize = 8192;
const MAX_CONNECTION_ATTEMPTS: u32 = 5;

pub struct NitroProver;

impl NitroProver {
    pub fn prove(input: GuestInput) -> ProverResult<Proof> {
        // let vsocket = vsock_connect(CID, PORT)?;
        // let fd = vsocket.as_raw_fd();
        let mut stream = VsockStream::connect(&VsockAddr::new(CID, PORT)).map_err(|e| {
            ProverError::GuestError(format!("Connection to VSoc failed with details {}", e))
        })?;

        let input_bytes = serde_json::to_string(&input)?;
        // send proof request
        send_message(&mut stream, input_bytes).map_err(|e| {
            ProverError::GuestError(format!(
                "Failed to send proof request to enclave with details {}",
                e
            ))
        })?;
        // read proof response
        let proof = recv_message(&mut stream).map_err(|e| {
            ProverError::GuestError(format!(
                "Failed to read proof from enclave with details {}",
                e
            ))
        })?;
        Ok(proof.into())
    }
}

impl Prover for NitroProver {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
    ) -> ProverResult<Proof> {
        // start tracing + logging
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        // read and validate inputs
        info!("Starting Nitro guest and proof generation");
        // read and validate inputs
        if !input.taiko.skip_verify_blob {
            warn!("blob verification skip. terminating");
            process::exit(1);
        }
        // process the block
        let (header, _mpt_node) = TaikoStrategy::build_from(&input)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        // calculate the public input hash
        let pi = ProtocolInstance::new(&input, &header, raiko_lib::consts::VerifierType::Nitro)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        let pi_hash = pi.instance_hash();
        info!(
            "Block {}. PI data to be signed {}",
            input.block_number, pi_hash
        );

        // Nitro prove of processed block
        let nsm_fd = nsm_init();

        let signing_key = generate_key();
        let public = signing_key.public_key();
        let signature = sign_message(&signing_key.secret_key(), pi_hash)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        let user_data = ByteBuf::from(signature.to_vec());

        let request = Request::Attestation {
            user_data: Some(user_data),
            nonce: None, // FIXME: shold this be some?
            public_key: Some(ByteBuf::from(public.serialize_uncompressed())), // use this provided key in doc to verify
        };
        let Response::Attestation { document: result } = nsm_process_request(nsm_fd, request)
        else {
            return Err(ProverError::GuestError(
                "Failed to collect attestation document".to_string(),
            ));
        };

        nsm_exit(nsm_fd);
        Ok(result.into())
    }
}

// Initiate a connection on an AF_VSOCK socket
// fn vsock_connect(cid: u32, port: u32) -> Result<VsockSocket, String> {
//     let sockaddr = VsockAddr::new(cid, port);
//     let mut err_msg = String::new();

//     for i in 0..MAX_CONNECTION_ATTEMPTS {
//         let vsocket = VsockSocket::new(
//             socket(
//                 AddressFamily::Vsock,
//                 SockType::Stream,
//                 SockFlag::empty(),
//                 None,
//             )
//             .map_err(|err| format!("Failed to create the socket: {:?}", err))?
//             .as_raw_fd(),
//         );
//         match connect(vsocket.as_raw_fd(), &sockaddr) {
//             Ok(_) => return Ok(vsocket),
//             Err(e) => err_msg = format!("Failed to connect: {}", e),
//         }

//         // Exponentially backoff before retrying to connect to the socket
//         std::thread::sleep(std::time::Duration::from_secs(1 << i));
//     }

//     Err(err_msg)
// }

// struct VsockSocket {
//     socket_fd: RawFd,
// }

// impl VsockSocket {
//     fn new(socket_fd: RawFd) -> Self {
//         VsockSocket { socket_fd }
//     }
// }

// impl Drop for VsockSocket {
//     fn drop(&mut self) {
//         shutdown(self.socket_fd, Shutdown::Both)
//             .unwrap_or_else(|e| eprintln!("Failed to shut socket down: {:?}", e));
//         close(self.socket_fd).unwrap_or_else(|e| eprintln!("Failed to close socket: {:?}", e));
//     }
// }

// impl AsRawFd for VsockSocket {
//     fn as_raw_fd(&self) -> RawFd {
//         self.socket_fd
//     }
// }
