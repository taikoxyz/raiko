use alloy_primitives::{hex, Address, ChainId, Signature, B256};
use alloy_signer::{utils::secret_key_to_address, Result as SignerResult, Signer, SignerSync};
use async_trait::async_trait;
use k256::{
    ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, SigningKey},
    FieldBytes,
};
use rand::{CryptoRng, Rng};
use std::{fmt, str::FromStr};

pub type PrivateKeySigner = LocalSigner<SigningKey>;

#[derive(Clone)]
pub struct LocalSigner<C> {
    credential: C,
    address: Address,
    chain_id: Option<ChainId>,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalSignerError {
    #[error(transparent)]
    EcdsaError(#[from] k256::ecdsa::Error),
    #[error(transparent)]
    HexError(#[from] hex::FromHexError),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

impl LocalSigner<SigningKey> {
    pub fn from_signing_key(credential: SigningKey) -> Self {
        let address = secret_key_to_address(&credential);
        Self::new_with_credential(credential, address, None)
    }

    pub fn from_field_bytes(bytes: &FieldBytes) -> Result<Self, k256::ecdsa::Error> {
        SigningKey::from_bytes(bytes).map(Self::from_signing_key)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, k256::ecdsa::Error> {
        SigningKey::from_slice(bytes).map(Self::from_signing_key)
    }

    pub fn random() -> Self {
        Self::random_with(&mut rand::thread_rng())
    }

    pub fn random_with<R: Rng + CryptoRng>(rng: &mut R) -> Self {
        Self::from_signing_key(SigningKey::random(rng))
    }
}

impl<C> LocalSigner<C> {
    pub const fn new_with_credential(
        credential: C,
        address: Address,
        chain_id: Option<ChainId>,
    ) -> Self {
        Self {
            credential,
            address,
            chain_id,
        }
    }

    pub const fn address(&self) -> Address {
        self.address
    }

    pub const fn chain_id(&self) -> Option<ChainId> {
        self.chain_id
    }
}

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
impl<C> Signer for LocalSigner<C>
where
    C: PrehashSigner<(k256::ecdsa::Signature, RecoveryId)> + Send + Sync,
{
    async fn sign_hash(&self, hash: &B256) -> SignerResult<Signature> {
        self.sign_hash_sync(hash)
    }

    fn address(&self) -> Address {
        self.address
    }

    fn chain_id(&self) -> Option<ChainId> {
        self.chain_id
    }

    fn set_chain_id(&mut self, chain_id: Option<ChainId>) {
        self.chain_id = chain_id;
    }
}

impl<C> SignerSync for LocalSigner<C>
where
    C: PrehashSigner<(k256::ecdsa::Signature, RecoveryId)>,
{
    fn sign_hash_sync(&self, hash: &B256) -> SignerResult<Signature> {
        Ok(self.credential.sign_prehash(hash.as_ref())?.into())
    }

    fn chain_id_sync(&self) -> Option<ChainId> {
        self.chain_id
    }
}

impl From<SigningKey> for LocalSigner<SigningKey> {
    fn from(value: SigningKey) -> Self {
        Self::from_signing_key(value)
    }
}

impl FromStr for LocalSigner<SigningKey> {
    type Err = LocalSignerError;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        let array = hex::decode_to_array::<_, 32>(src)?;
        Ok(Self::from_slice(&array)?)
    }
}

impl<C> fmt::Debug for LocalSigner<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalSigner")
            .field("address", &self.address)
            .field("chain_id", &self.chain_id)
            .finish()
    }
}
