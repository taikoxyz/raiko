use crate::commitment_to_version_hash;
use crate::input::GuestInput;
use kzg::eip_4844::{
    blob_to_polynomial, compute_challenge, compute_kzg_proof_rust,
    evaluate_polynomial_in_evaluation_form, hash_to_bls_field, Blob,
};
use kzg::{Fr, G1};
use once_cell::sync::Lazy;
use revm_primitives::{
    kzg::{G1Points, G2Points, G1_POINTS, G2_POINTS},
    B256,
};
use sha2::{Digest as _, Sha256};
use std::sync::{Arc, RwLock};

#[cfg(feature = "kzg-zkcrypto")]
mod backend_exports {
    pub use kzg::eip_4844::blob_to_kzg_commitment_rust;
    pub use rust_kzg_zkcrypto::eip_4844::deserialize_blob_rust;
    pub use rust_kzg_zkcrypto::kzg_proofs::KZGSettings as TaikoKzgSettings;
}
pub use backend_exports::*;

pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;
pub static MAINNET_KZG_TRUSTED_SETUP: Lazy<Arc<TaikoKzgSettings>> = Lazy::new(|| {
    Arc::new(
        kzg::eip_4844::load_trusted_setup_rust(
            G1Points::as_ref(G1_POINTS).flatten(),
            G2Points::as_ref(G2_POINTS).flatten(),
        )
        .expect("failed to load trusted setup"),
    )
});

pub static mut VERSION_HASH_AND_PROOF: Lazy<RwLock<(B256, KzgGroup)>> =
    Lazy::new(|| RwLock::new((B256::default(), [0u8; 48])));

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
    SetCommitmentProof(String),
}

pub fn proof_of_equivalence(input: &GuestInput) -> Result<(KzgField, KzgField), Eip4844Error> {
    let blob = &input.taiko.tx_data;
    let kzg_settings = input.taiko.kzg_settings.as_ref().unwrap_or_else(|| {
        // very costly, should not happen
        println!("initializing kzg settings in prover");
        &*MAINNET_KZG_TRUSTED_SETUP
    });

    let blob_fields = Blob::from_bytes(blob)
        .and_then(|b| deserialize_blob_rust(&b))
        .map_err(|_| Eip4844Error::DeserializeBlob)?;

    let poly = blob_to_polynomial(&blob_fields).unwrap();
    let blob_hash = Sha256::digest(blob).into();

    let x = hash_to_bls_field(&blob_hash);
    let y = evaluate_polynomial_in_evaluation_form(&poly, &x, kzg_settings)
        .map(|fr| fr.to_bytes())
        .map_err(|e| Eip4844Error::EvaluatePolynomial(e.to_string()))?;

    Ok((x.to_bytes(), y))
}

pub fn proof_of_version_hash(input: &GuestInput) -> Result<Option<B256>, Eip4844Error> {
    if input.taiko.skip_verify_blob {
        Ok(None)
    } else {
        let blob_fields = Blob::from_bytes(&input.taiko.tx_data)
            .and_then(|b| deserialize_blob_rust(&b))
            .map_err(|_| Eip4844Error::DeserializeBlob)?;

        let kzg_settings = input
            .taiko
            .kzg_settings
            .as_ref()
            .unwrap_or_else(|| &*MAINNET_KZG_TRUSTED_SETUP);
        let commitment = blob_to_kzg_commitment_rust(&blob_fields, kzg_settings)
            .map_err(Eip4844Error::ComputeKzgProof)?;
        Ok(Some(commitment_to_version_hash(&commitment.to_bytes())))
    }
}

pub fn get_kzg_proof_commitment(
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

pub fn set_commitment_proof(proof: &KzgGroup, commitment: &KzgGroup) -> Result<(), Eip4844Error> {
    let version_hash = commitment_to_version_hash(commitment);
    unsafe {
        *VERSION_HASH_AND_PROOF
            .write()
            .map_err(|e| Eip4844Error::SetCommitmentProof(e.to_string()))? = (version_hash, *proof);
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use std::io::Read;

    use super::*;
    use kzg::eip_4844::{load_trusted_setup_rust, verify_kzg_proof_rust, BYTES_PER_FIELD_ELEMENT};
    use kzg::G1;
    use lazy_static::lazy_static;
    use revm_primitives::kzg::parse_kzg_trusted_setup;
    use revm_primitives::Bytes;
    use rust_kzg_zkcrypto::kzg_types::ZG1;

    lazy_static! {
        // "./lib/trusted_setup.txt"
        static ref POINTS: (Box<G1Points>, Box<G2Points>) =  std::fs::read_to_string("trusted_setup.txt")
            .map(|s| parse_kzg_trusted_setup(&s).expect("failed to parse kzg trusted setup"))
            .expect("failed to kzg_parsed_trust_setup.bin");
    }

    #[test]
    fn test_parse_kzg_trusted_setup() {
        println!("g1: {:?}", POINTS.0.len());
        println!("g2: {:?}", POINTS.1.len());

        assert_eq!(
            POINTS.0.len(),
            MAINNET_KZG_TRUSTED_SETUP.as_ref().secret_g1.len()
        );
        assert_eq!(
            POINTS.1.len(),
            MAINNET_KZG_TRUSTED_SETUP.as_ref().secret_g2.len()
        );
    }

    #[test]
    fn test_blob_to_kzg_commitment() {
        let kzg_settings: TaikoKzgSettings = load_trusted_setup_rust(
            G1Points::as_ref(&POINTS.0).flatten(),
            G2Points::as_ref(&POINTS.1).flatten(),
        )
        .unwrap();
        let blob = Blob::from_bytes(&[0u8; 131072]).unwrap();
        let commitment =
            blob_to_kzg_commitment_rust(&deserialize_blob_rust(&blob).unwrap(), &kzg_settings)
                .map(|c| c.to_bytes())
                .unwrap();
        assert_eq!(
            commitment_to_version_hash(&commitment).to_string(),
            "0x010657f37554c781402a22917dee2f75def7ab966d7b770905398eba3c444014"
        );
    }

    #[test]
    fn test_verify_kzg_proof() {
        let kzg_settings: TaikoKzgSettings = load_trusted_setup_rust(
            G1Points::as_ref(&POINTS.0).flatten(),
            G2Points::as_ref(&POINTS.1).flatten(),
        )
        .unwrap();
        let blob = Blob::from_bytes(&[0u8; 131072]).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let (proof, commitment) = get_kzg_proof_commitment(&blob.bytes, &kzg_settings).unwrap();
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
        let kzg_settings: TaikoKzgSettings = load_trusted_setup_rust(
            G1Points::as_ref(&POINTS.0).flatten(),
            G2Points::as_ref(&POINTS.1).flatten(),
        )
        .unwrap();
        let blob = Blob::from_bytes(&[0u8; 131072]).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let (proof, commitment) = get_kzg_proof_commitment(&blob.bytes, &kzg_settings).unwrap();
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
