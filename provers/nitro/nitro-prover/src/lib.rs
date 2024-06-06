use aws_nitro_enclaves_nsm_api::{
    api::{Request, Response},
    driver::{nsm_exit, nsm_init, nsm_process_request},
};
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{Proof, Prover, ProverConfig, ProverError, ProverResult},
};
use serde_bytes::ByteBuf;

pub struct NitroProver;

impl Prover for NitroProver {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let nsm_fd = nsm_init();

        let user_data = ByteBuf::from(
            bincode::serialize(&input).map_err(|e| ProverError::GuestError(e.to_string()))?,
        );

        let request = Request::Attestation {
            user_data: Some(user_data),
            nonce: None,      // FIXME: shold this be some?
            public_key: None, // we use provided key in doc to sign if required
        };
        let Response::Attestation { document: result } = nsm_process_request(nsm_fd, request)
        else {
            return Err(ProverError::GuestError(
                "Failed to collect attestation document".to_string(),
            ));
        };

        // let _pub_key = AttestationDoc::from_binary(&result)
        //     .map_err(|e| ProverError::GuestError(format!("{e:?}")))?
        //     .public_key
        //     .ok_or(ProverError::GuestError(
        //         "No Public Key attached to report".to_string(),
        //     ))?;

        nsm_exit(nsm_fd);
        Ok(result.into())
    }
}
