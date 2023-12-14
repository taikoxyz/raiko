use std::{fs, path::Path};

use rand_core::OsRng;
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Error, KeyPair, Message, PublicKey, Secp256k1, SecretKey, SECP256K1,
};
use zeth_primitives::{keccak256, signature::TxSignature, Address, B256, U256};

pub fn generate_key() -> KeyPair {
    KeyPair::new(&Secp256k1::new(), &mut OsRng)
}

/// Recovers the address of the sender using secp256k1 pubkey recovery.
///
/// Converts the public key into an ethereum address by hashing the public key with
/// keccak256.
///
/// This does not ensure that the `s` value in the signature is low, and _just_ wraps the
/// underlying secp256k1 library.
pub fn recover_signer_unchecked(sig: &[u8; 65], msg: &[u8; 32]) -> Result<Address, Error> {
    let sig =
        RecoverableSignature::from_compact(&sig[0..64], RecoveryId::from_i32(sig[64] as i32)?)?;

    let public = SECP256K1.recover_ecdsa(&Message::from_slice(&msg[..32])?, &sig)?;
    Ok(public_key_to_address(public))
}

/// Signs message with the given secret key.
/// Returns the corresponding signature.
pub fn sign_message(secret_key: &SecretKey, message: B256) -> Result<TxSignature, Error> {
    let secret = B256::from_slice(&secret_key.secret_bytes()[..]);
    let sec = SecretKey::from_slice(secret.as_ref())?;
    let s = SECP256K1.sign_ecdsa_recoverable(&Message::from_slice(&message[..])?, &sec);
    let (rec_id, data) = s.serialize_compact();

    let signature = TxSignature {
        r: U256::try_from_be_slice(&data[..32]).expect("The slice has at most 32 bytes"),
        s: U256::try_from_be_slice(&data[32..64]).expect("The slice has at most 32 bytes"),
        v: (rec_id.to_i32() != 0) as u64,
    };
    Ok(signature)
}

/// Converts a public key into an ethereum address by hashing the encoded public key with
/// keccak256.
pub fn public_key_to_address(public: PublicKey) -> Address {
    // strip out the first byte because that should be the SECP256K1_TAG_PUBKEY_UNCOMPRESSED
    // tag returned by libsecp's uncompressed pubkey serialization
    let hash = keccak256(&public.serialize_uncompressed()[1..]);
    Address::from_slice(&hash[12..])
}

pub fn load_private_key<T: AsRef<Path>>(path: T) -> Result<SecretKey, Error> {
    let data = fs::read(path).unwrap();
    SecretKey::from_slice(data.as_ref())
}

pub fn public_key(secret: &SecretKey) -> PublicKey {
    PublicKey::from_secret_key(&Secp256k1::new(), secret)
}
