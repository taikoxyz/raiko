// #![cfg(feature = "kzg")]

use core::fmt::Display;
use std::sync::{Arc, RwLock};
use once_cell::sync::Lazy;
use revm_primitives::{kzg::{G1Points, G2Points, G1_POINTS, G2_POINTS}, B256};
use sha2::{Digest as _, Sha256};
use kzg::eip_4844::{
    compute_challenge, compute_kzg_proof_rust,
    blob_to_kzg_commitment_rust, blob_to_polynomial, compute_blob_kzg_proof_rust, compute_kzg_proof_rust, evaluate_polynomial_in_evaluation_form, hash_to_bls_field, Blob
};

#[cfg(feature = "kzg-zkcrypto")]
mod backend_exports {
    pub use rust_kzg_zkcrypto::kzg_proofs::KZGSettings as TaikoKzgSettings;
    pub use rust_kzg_zkcrypto::eip_4844::deserialize_blob_rust;
}

pub use backend_exports::*;

use crate::input::GuestInput;

pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;
pub static MAINNET_KZG_TRUSTED_SETUP: Lazy<Arc<TaikoKzgSettings>> = 
    Lazy::new(|| {
        Arc::new(
            kzg::eip_4844::load_trusted_setup_rust(
                G1Points::as_ref(&G1_POINTS).flatten(), 
                G2Points::as_ref(&G2_POINTS).flatten()
            )
            .expect("failed to load trusted setup"),
        )
    });

pub static mut VERSION_HASH_AND_PROOF: Lazy<RwLock<(B256, Vec<u8>)>> = 
    Lazy::new(|| RwLock::new((B256::default(), vec![])));


#[derive(Debug, thiserror::Error)]
pub enum Eip4844Error {
    #[error("Failed to deserialize blob to field elements")]
    DeserializeBlobError,
    #[error("Failed to evaluate polynomial at hashed point: {0}")]
    EvaluatePolynomialError(String),
    #[error("Failed to compute KZG proof")]
    ComputeKzgProofError(String),
    #[error("Failed set commitment proof")]
    SetCommitmentProofError(String),
}

pub fn proof_of_equivalence(input: &GuestInput) -> Result<Option<Vec<u8>>, Eip4844Error> {
    if input.taiko.skip_verify_blob {
        return Ok(None);
    } else {
        let blob = &input.taiko.tx_data;
        let kzg_settings = input.taiko.kzg_setting.as_ref().unwrap_or_else(|| {
            // very costly, should not happen
            println!("initializing kzg settings in prover"); 
            &*MAINNET_KZG_TRUSTED_SETUP
        });
        Ok(Some(proof_of_equivalence_eval(blob, kzg_settings)?))
    }
}

pub fn proof_of_version_hash(input: &GuestInput) -> Result<Option<B256>, Eip4844Error> {
    if input.taiko.skip_verify_blob {
        return Ok(None);
    } else {
        let blob = &input.taiko.tx_data;
        let kzg_settings = input.taiko.kzg_setting.as_ref().unwrap_or_else(|| &*MAINNET_KZG_TRUSTED_SETUP);
        let (_, y) = get_kzg_proof(blob, kzg_settings)?;
        Ok(Some(commitment_to_version_hash(&y)))
    }
}

pub fn proof_of_equivalence_eval(blob: &[u8], kzg_settings: &TaikoKzgSettings) -> Result<Vec<u8>, Eip4844Error> {

    let blob_fields = Blob::from_bytes(blob)
        .map(|b| deserialize_blob_rust(&b))
        .flatten()
        .expect("Failed to deserialize blob to field elements");

    let poly = blob_to_polynomial(&blob_fields).unwrap();
    let blob_hash = Sha256::digest(blob).into();
    let x = hash_to_bls_field(&blob_hash);
    
    // y = poly(x)
    evaluate_polynomial_in_evaluation_form(&poly, &x, kzg_settings)
        .map(|fr| bincode::serialize(&fr).unwrap())
        .map_err(|e| Eip4844Error::EvaluatePolynomialError(e))
}

pub fn get_kzg_proof(blob: &[u8], kzg_settings: &TaikoKzgSettings) -> Result<(Vec<u8>, Vec<u8>), Eip4844Error> {
    let blob_fields = Blob::from_bytes(blob)
        .map(|b| deserialize_blob_rust(&b))
        .flatten()
        .expect("Failed to deserialize blob to field elements");

    let commitment = blob_to_kzg_commitment_rust(&blob_fields, kzg_settings)
        .map_err(|e| Eip4844Error::ComputeKzgProofError(e))?;

    let evaluation_challenge_fr = compute_challenge(&blob_fields, &commitment);
    let (proof, y) = compute_kzg_proof_rust(&blob_fields, &evaluation_challenge_fr, kzg_settings)
        .map(|(proof, y)| (bincode::serialize(&proof).unwrap(), bincode::serialize(&y).unwrap()))
        .map_err(|e| Eip4844Error::ComputeKzgProofError(e))?;

    Ok((proof, y))
}


pub fn set_commitment_proof(proof: Vec<u8>, commitment: Vec<u8>) -> Result<(), Eip4844Error> {
    let version_hash = commitment_to_version_hash(&commitment);
    unsafe {
        *VERSION_HASH_AND_PROOF
            .write()
            .map_err(|e| Eip4844Error::SetCommitmentProofError(e.to_string()))?
        = (version_hash, proof);
    }
    Ok(())
}

pub fn commitment_to_version_hash(commitment: &[u8]) -> B256 {
    let mut hash = Sha256::digest(commitment);
    hash[0] = VERSIONED_HASH_VERSION_KZG;
    B256::new(hash.into())
}
