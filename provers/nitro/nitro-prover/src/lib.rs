use crate::protocol_helper::*;
use anyhow::{bail, Result};
use aws_nitro_enclaves_nsm_api::{
    api::{Request, Response},
    driver::{nsm_exit, nsm_init, nsm_process_request},
};
use raiko_lib::{
    builder::calculate_block_header,
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{IdWrite, Proof, Prover, ProverConfig, ProverError, ProverResult},
    signature::sign_message,
};
use secp256k1::{Keypair, SECP256K1};
use serde_bytes::ByteBuf;
use tracing::{debug, info};
use vsock::{VsockAddr, VsockStream};

pub mod protocol_helper;

pub const CID: u32 = 16;
pub const PORT: u32 = 26000;
pub const NON_HEX_PREFIX: &str = "XYZ";

const SECRET_LOCATION: &str = "/raiko-nitro/secret.key";

pub struct NitroProver;

impl NitroProver {
    pub fn load_key() -> Result<Keypair> {
        let Ok(key_data) = std::fs::read(SECRET_LOCATION) else {
            bail!("No SK found.");
        };
        Ok(Keypair::from_seckey_slice(
            SECP256K1,
            &hex::decode(key_data)?,
        )?)
    }
    pub fn get_attestation() -> Result<Vec<u8>> {
        let Ok(key) = Self::load_key() else {
            bail!("Non initialized enclave");
        };
        // Nitro prove of processed block
        let nsm_fd = nsm_init();

        let public = key.public_key();

        let request = Request::Attestation {
            user_data: None,
            nonce: None,
            public_key: Some(ByteBuf::from(public.serialize_uncompressed())), // use this provided key in doc to verify
        };
        let Response::Attestation { document: result } = nsm_process_request(nsm_fd, request)
        else {
            bail!("Failed to collect attestation document".to_string());
        };

        nsm_exit(nsm_fd);
        Ok(result)
    }
    pub fn prove(input: GuestInput) -> ProverResult<Proof> {
        debug!("Starting VSock for nitro proof enclave communication");
        let mut stream = VsockStream::connect(&VsockAddr::new(CID, PORT)).map_err(|e| {
            ProverError::GuestError(format!("Connection to VSoc failed with details {}", e))
        })?;

        let input_bytes = serde_json::to_string(&input)?;
        // send proof request
        debug!("Sending input to enclave");
        send_message(&mut stream, input_bytes).map_err(|e| {
            ProverError::GuestError(format!(
                "Failed to send proof request to enclave with details {}",
                e
            ))
        })?;
        // read proof response
        debug!("Reading proof from enclave");
        let proving_result = recv_message(&mut stream).map_err(|e| {
            ProverError::GuestError(format!(
                "Failed to read proof from enclave with details {}",
                e
            ))
        })?;
        if proving_result.starts_with(NON_HEX_PREFIX) {
            return Err(ProverError::GuestError(
                proving_result
                    .trim_start_matches(NON_HEX_PREFIX)
                    .to_string(),
            ));
        }
        debug!("Proof acquired. Returning it.");
        Ok(Proof {
            quote: Some(proving_result),
            ..Default::default()
        })
    }
}

impl Prover for NitroProver {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // read and validate inputs
        info!("Starting Nitro guest and proof generation");
        // process the block
        let header =
            calculate_block_header(&input).map_err(|e| ProverError::GuestError(e.to_string()))?;
        // calculate the public input hash
        let pi = ProtocolInstance::new(&input, &header, raiko_lib::consts::VerifierType::Nitro)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        let pi_hash = pi.instance_hash();
        info!(
            "Block {}. PI data to be signed {}",
            input.block.header.number, pi_hash
        );

        let signing_key = Self::load_key().map_err(|e| ProverError::GuestError(e.to_string()))?;
        let signature = sign_message(&signing_key.secret_key(), pi_hash)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        let user_data = ByteBuf::from(signature.to_vec());

        info!("Successfully generated proof for PI {}", pi_hash);
        Ok(Proof {
            proof: Some(hex::encode(user_data)),
            quote: Some(hex::encode(
                Self::get_attestation().map_err(|e| ProverError::GuestError(e.to_string()))?,
            )),
            ..Default::default()
        })
    }

    async fn cancel(
        _proof_key: raiko_lib::prover::ProofKey,
        _read: Box<&mut dyn raiko_lib::prover::IdStore>,
    ) -> ProverResult<()> {
        Ok(())
    }
}
