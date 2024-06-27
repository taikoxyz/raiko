use crate::input::GuestInput;
use kzg::eip_4844::{
    blob_to_polynomial, compute_challenge, compute_kzg_proof_rust,
    evaluate_polynomial_in_evaluation_form, hash_to_bls_field, Blob,
};
use kzg::{Fr, G1};
use once_cell::sync::Lazy;
use revm_primitives::kzg::{G1Points, G2Points, G1_POINTS, G2_POINTS};
use sha2::{Digest as _, Sha256};
use std::sync::RwLock;

#[cfg(feature = "kzg-zkcrypto")]
mod backend_exports {
    pub use kzg::eip_4844::blob_to_kzg_commitment_rust;
    pub use rust_kzg_zkcrypto::eip_4844::deserialize_blob_rust;
    pub use rust_kzg_zkcrypto::kzg_proofs::KZGSettings as TaikoKzgSettings;
    pub static TAIKO_KZG_SETTINGS_BIN: &[u8] =
        include_bytes!("../../kzg_settings/zkcrypto_kzg_settings.bin");
}
pub use backend_exports::*;

/// The KZG settings under the concrete type of kzg backend
/// We directly include the serialzed struct to avoid conversion cost in guest
/// To generate the bytes, run:
///
///     cargo run --bin gen_kzg_settings
///
/// If TAIKO_KZG_SETTINGS_BIN does not eixst, convert from the revm trusted setup
pub static TAIKO_KZG_SETTINGS: Lazy<TaikoKzgSettings> = Lazy::new(|| {
    bincode::deserialize(TAIKO_KZG_SETTINGS_BIN).unwrap_or(
        kzg::eip_4844::load_trusted_setup_rust(
            G1Points::as_ref(G1_POINTS).flatten(),
            G2Points::as_ref(G2_POINTS).flatten(),
        )
        .expect("failed to load trusted setup"),
    )
});

pub static mut COMMITMENT_AND_PROOF: Lazy<RwLock<(KzgGroup, KzgGroup)>> =
    Lazy::new(|| RwLock::new(([0u8; 48], [0u8; 48])));

pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;

pub type KzgGroup = [u8; 48];
pub type KzgField = [u8; 32];

#[derive(Debug, thiserror::Error)]
pub enum Eip4844Error {
    #[error("Failed to deserialize blob to field elements")]
    DeserializeBlob,
    #[error("Failed to evaluate polynomial at hashed point: {0}")]
    EvaluatePolynomial(String),
    #[error("Failed to compute KZG proof")]
    ComputeKzgProof(String),
    #[error("Failed set commitment proof")]
    KzgDataPoison(String),
}

pub fn proof_of_equivalence(input: &GuestInput) -> Result<(KzgField, KzgField), Eip4844Error> {
    let blob = &input.taiko.tx_data;
    let blob_fields = Blob::from_bytes(blob)
        .and_then(|b| deserialize_blob_rust(&b))
        .map_err(|_| Eip4844Error::DeserializeBlob)?;

    let poly = blob_to_polynomial(&blob_fields).unwrap();
    let blob_hash = Sha256::digest(blob).into();

    let x = hash_to_bls_field(&blob_hash);
    let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &TAIKO_KZG_SETTINGS.clone())
        .map(|fr| fr.to_bytes())
        .map_err(|e| Eip4844Error::EvaluatePolynomial(e.to_string()))?;

    Ok((x.to_bytes(), y))
}

pub fn proof_of_commitment(input: &GuestInput) -> Result<KzgGroup, Eip4844Error> {
    let blob_fields = Blob::from_bytes(&input.taiko.tx_data)
        .and_then(|b| deserialize_blob_rust(&b))
        .map_err(|_| Eip4844Error::DeserializeBlob)?;

    blob_to_kzg_commitment_rust(&blob_fields, &TAIKO_KZG_SETTINGS.clone())
        .map(|commmitment| commmitment.to_bytes())
        .map_err(Eip4844Error::ComputeKzgProof)
}

pub fn calc_kzg_proof_commitment(
    blob: &[u8],
    kzg_settings: &TaikoKzgSettings,
) -> Result<(KzgGroup, KzgGroup), Eip4844Error> {
    let blob_fields = Blob::from_bytes(blob)
        .and_then(|b| deserialize_blob_rust(&b))
        .map_err(|_| Eip4844Error::DeserializeBlob)?;

    let commitment = blob_to_kzg_commitment_rust(&blob_fields, kzg_settings)
        .map_err(Eip4844Error::ComputeKzgProof)?;

    let evaluation_challenge_fr = compute_challenge(&blob_fields, &commitment);
    let (proof, _) = compute_kzg_proof_rust(&blob_fields, &evaluation_challenge_fr, kzg_settings)
        .map_err(Eip4844Error::ComputeKzgProof)?;

    Ok((proof.to_bytes(), commitment.to_bytes()))
}

pub fn save_cur_blob_proof(commitment: &KzgGroup, proof: &KzgGroup) -> Result<(), Eip4844Error> {
    unsafe {
        *COMMITMENT_AND_PROOF
            .write()
            .map_err(|e| Eip4844Error::KzgDataPoison(e.to_string()))? = (*commitment, *proof);
    }
    Ok(())
}

pub fn load_cur_blob_proof() -> Result<(KzgGroup, KzgGroup), Eip4844Error> {
    unsafe {
        COMMITMENT_AND_PROOF
            .read()
            .map_err(|e| Eip4844Error::KzgDataPoison(e.to_string()))
            .map(|r| *r)
    }
}

#[cfg(test)]
mod test {
    use std::io::Read;

    use super::*;
    use kzg::eip_4844::{verify_kzg_proof_rust, BYTES_PER_FIELD_ELEMENT};
    use kzg::G1;

    use crate::commitment_to_version_hash;
    use revm_primitives::Bytes;
    use rust_kzg_zkcrypto::kzg_types::ZG1;

    #[test]
    fn test_kzg_settings_equivalence() {
        let kzg_settings: TaikoKzgSettings = kzg::eip_4844::load_trusted_setup_rust(
            G1Points::as_ref(G1_POINTS).flatten(),
            G2Points::as_ref(G2_POINTS).flatten(),
        )
        .expect("failed to load trusted setup");
        assert_eq!(TAIKO_KZG_SETTINGS.clone().secret_g1, kzg_settings.secret_g1);
        assert_eq!(TAIKO_KZG_SETTINGS.clone().secret_g2, kzg_settings.secret_g2);
    }

    #[test]
    fn test_blob_to_kzg_commitment() {
        let blob = Blob::from_bytes(&[0u8; 131072]).unwrap();
        let commitment = blob_to_kzg_commitment_rust(
            &deserialize_blob_rust(&blob).unwrap(),
            &TAIKO_KZG_SETTINGS.clone(),
        )
        .map(|c| c.to_bytes())
        .unwrap();
        assert_eq!(
            commitment_to_version_hash(&commitment).to_string(),
            "0x010657f37554c781402a22917dee2f75def7ab966d7b770905398eba3c444014"
        );
    }

    #[test]
    fn test_verify_kzg_proof() {
        let kzg_settings = TAIKO_KZG_SETTINGS.clone();
        let blob = Blob::from_bytes(&[0u8; 131072]).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let (proof, commitment) = calc_kzg_proof_commitment(&blob.bytes, &kzg_settings).unwrap();
        let poly = blob_to_polynomial(&blob_fields).unwrap();

        // Random number hash to field
        let x = hash_to_bls_field(&[5; BYTES_PER_FIELD_ELEMENT]);
        let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &kzg_settings).unwrap();

        verify_kzg_proof_rust(
            &ZG1::from_bytes(&commitment).unwrap(),
            &x,
            &y,
            &ZG1::from_bytes(&proof).unwrap(),
            &kzg_settings,
        )
        .unwrap();
    }

    #[test]
    fn test_verify_kzg_proof_in_precompile() {
        let kzg_settings = TAIKO_KZG_SETTINGS.clone();
        let blob = Blob::from_bytes(&[0u8; 131072]).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let (proof, commitment) = calc_kzg_proof_commitment(&blob.bytes, &kzg_settings).unwrap();
        let poly = blob_to_polynomial(&blob_fields).unwrap();

        // Random number hash to field
        let x = hash_to_bls_field(&[5; BYTES_PER_FIELD_ELEMENT]);
        let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &kzg_settings).unwrap();

        // The input is encoded as follows:
        // | versioned_hash |  z  |  y  | commitment | proof |
        // |     32         | 32  | 32  |     48     |   48  |
        let version_hash = commitment_to_version_hash(&commitment);
        let mut input = [0u8; 192];
        input[..32].copy_from_slice(&(*version_hash));
        input[32..64].copy_from_slice(&x.to_bytes());
        input[64..96].copy_from_slice(&y.to_bytes());
        input[96..144].copy_from_slice(&commitment);
        input[144..192].copy_from_slice(&proof);

        revm_precompile::kzg_point_evaluation::run(
            &Bytes::copy_from_slice(&input),
            u64::MAX,
            &revm_primitives::env::Env::default(),
        )
        .unwrap();
    }
}
