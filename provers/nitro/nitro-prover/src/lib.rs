use aws_nitro_enclaves_nsm_api::{
    api::{Request, Response},
    driver::{nsm_exit, nsm_init, nsm_process_request},
};
use once_cell::sync::Lazy;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{Proof, Prover, ProverConfig, ProverError, ProverResult},
};
use serde_bytes::ByteBuf;
use std::{env, fs::read, path::PathBuf};
use tokio::sync::OnceCell;

pub const PRIV_KEY_FILENAME: &str = "priv.key";

static PRIVATE_KEY: Lazy<OnceCell<PathBuf>> = Lazy::new(OnceCell::new);

pub struct NitroProver;

impl Prover for NitroProver {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let nsm_fd = nsm_init();
        let mut cur_dir = env::current_exe()
            .map_err(|_| ProverError::GuestError("Fail to get current directory".to_string()))?
            .parent()
            .ok_or(ProverError::GuestError(
                "Failed to get parent dir of executable".to_string(),
            ))?
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir
                .parent()
                .ok_or(ProverError::GuestError(
                    "Failed to get parent dir of executable".to_string(),
                ))?
                .to_path_buf();
        }

        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        let public_key = ByteBuf::from(
            read(PRIVATE_KEY.get().ok_or(ProverError::GuestError(
                "Key was initialized but failed to retrieve!".to_string(),
            ))?)
            .map_err(|e| ProverError::GuestError(e.to_string()))?,
        );

        let user_data = ByteBuf::from(
            bincode::serialize(&input).map_err(|e| ProverError::GuestError(e.to_string()))?,
        );

        let request = Request::Attestation {
            user_data: Some(user_data),
            nonce: None, // FIXME: shold this be some?
            public_key: Some(public_key),
        };
        let result = match nsm_process_request(nsm_fd, request) {
            Response::Attestation { document } => document,
            _ => unreachable!(),
        };
        nsm_exit(nsm_fd);
        Ok(result.into())
    }
}
