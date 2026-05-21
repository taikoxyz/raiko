use alloy_primitives::{Address, ChainId, Signature, B256};
use alloy_signer::{Error as SignerError, Signer};
use async_trait::async_trait;
use aws_sdk_kms::{
    error::SdkError,
    operation::{
        get_public_key::GetPublicKeyError,
        sign::{SignError, SignOutput},
    },
    Client,
};

#[derive(Clone, Debug)]
pub struct AwsSigner {
    chain_id: Option<ChainId>,
}

#[derive(Debug, thiserror::Error)]
pub enum AwsSignerError {
    #[error(transparent)]
    Sign(#[from] SdkError<SignError>),
    #[error(transparent)]
    GetPublicKey(#[from] SdkError<GetPublicKeyError>),
    #[error("AWS KMS signing is not enabled in this raiko build")]
    Unsupported,
    #[error("signature not found in response")]
    SignatureNotFound,
}

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
impl Signer for AwsSigner {
    async fn sign_hash(&self, _hash: &B256) -> alloy_signer::Result<Signature> {
        Err(SignerError::message("AWS KMS signing is not enabled in this raiko build"))
    }

    fn address(&self) -> Address {
        Address::ZERO
    }

    fn chain_id(&self) -> Option<ChainId> {
        self.chain_id
    }

    fn set_chain_id(&mut self, chain_id: Option<ChainId>) {
        self.chain_id = chain_id;
    }
}

impl AwsSigner {
    pub async fn new(
        _kms: Client,
        _key_id: String,
        chain_id: Option<ChainId>,
    ) -> Result<Self, AwsSignerError> {
        let _ = chain_id;
        Err(AwsSignerError::Unsupported)
    }

    pub async fn sign_digest_with_key(
        &self,
        _key_id: String,
        _digest: &B256,
    ) -> Result<SignOutput, AwsSignerError> {
        Err(AwsSignerError::Unsupported)
    }
}
