use aws_nitro_enclaves_nsm_api::{
    api::{Request, Response},
    driver::{nsm_exit, nsm_init, nsm_process_request},
};
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{Proof, Prover, ProverConfig, ProverError, ProverResult},
    signature::{generate_key, sign_message},
};
use serde_bytes::ByteBuf;
use std::process;
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

pub mod protocol_helper;

pub struct NitroProver;

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
