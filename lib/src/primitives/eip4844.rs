use alloy_consensus::Blob;
use alloy_primitives::B256;
use kzg::kzg_types::{ZFr, ZG1};
use kzg_traits::{
    eip_4844::{
        blob_to_kzg_commitment_rust, blob_to_polynomial, compute_kzg_proof_rust,
        evaluate_polynomial_in_evaluation_form, hash_to_bls_field, verify_kzg_proof_rust,
        BYTES_PER_FIELD_ELEMENT,
    },
    Fr, G1,
};
use once_cell::sync::Lazy;
use sha2::{Digest as _, Sha256};

pub use kzg::kzg_proofs::KZGSettings;

// Pull in the auto-generated file from OUT_DIR
pub mod trusted_setup_gen {
    include!(concat!(env!("OUT_DIR"), "/trusted_setup_gen.rs"));
}

// The KZG settings under the concrete type of kzg backend
// We directly include the serialzed struct generated from build.rs to avoid conversion cost in guest
pub static KZG_SETTINGS: Lazy<KZGSettings> = Lazy::new(|| trusted_setup_gen::prebuilt_settings());

pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;

pub type KzgGroup = [u8; 48];
pub type KzgField = [u8; 32];
pub type KzgCommitment = KzgGroup;

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
    #[error("Cannot convert &[u8] to Blob")]
    BlobConversion,
}

pub fn get_evaluation_point(blob: &[u8], versioned_hash: &B256) -> ZFr {
    let blob_hash = Sha256::digest(blob);
    let x = Sha256::digest([blob_hash.to_vec(), versioned_hash.to_vec()].concat()).into();
    hash_to_bls_field(&x)
}

/// Returns the evaluation point as raw bytes (avoids callers needing the `Fr` trait).
pub fn get_evaluation_point_bytes(blob: &[u8], versioned_hash: &B256) -> KzgField {
    get_evaluation_point(blob, versioned_hash).to_bytes()
}

pub fn deserialize_blob_rust(blob: &Blob) -> Result<Vec<ZFr>, String> {
    blob.0
        .chunks(BYTES_PER_FIELD_ELEMENT)
        .map(|chunk| {
            let mut bytes = [0u8; BYTES_PER_FIELD_ELEMENT];
            bytes.copy_from_slice(chunk);
            if let Ok(result) = ZFr::from_bytes(&bytes) {
                Ok(result)
            } else {
                Err("Failed to deserialized blob into field elements ZFr".to_string())
            }
        })
        .collect::<Result<Vec<ZFr>, String>>()
}

pub fn proof_of_equivalence(
    blob_bytes: &[u8],
    versioned_hash: &B256,
) -> Result<(KzgField, KzgField), Eip4844Error> {
    let blob = Blob::try_from(blob_bytes).map_err(|_| Eip4844Error::BlobConversion)?;

    let blob_fields = deserialize_blob_rust(&blob).map_err(|_| Eip4844Error::DeserializeBlob)?;

    let poly = blob_to_polynomial(&blob_fields).unwrap();
    let x = get_evaluation_point(blob_bytes, versioned_hash);
    let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &*KZG_SETTINGS)
        .map(|fr| fr.to_bytes())
        .map_err(|e| Eip4844Error::EvaluatePolynomial(e.to_string()))?;

    Ok((x.to_bytes(), y))
}

pub fn verify_kzg_proof_impl(
    commitment: KzgGroup,
    x: KzgField,
    y: KzgField,
    proof: KzgGroup,
) -> Result<bool, Eip4844Error> {
    let commitment = ZG1::from_bytes(&commitment).map_err(|e| Eip4844Error::ComputeKzgProof(e))?;
    let x = ZFr::from_bytes(&x).map_err(|e| Eip4844Error::ComputeKzgProof(e))?;
    let y = ZFr::from_bytes(&y).map_err(|e| Eip4844Error::ComputeKzgProof(e))?;
    let proof = ZG1::from_bytes(&proof).map_err(|e| Eip4844Error::ComputeKzgProof(e))?;

    verify_kzg_proof_rust(&commitment, &x, &y, &proof, &*KZG_SETTINGS)
        .map_err(|e| Eip4844Error::ComputeKzgProof(e))
}

pub fn calc_kzg_proof(blob: &[u8], versioned_hash: &B256) -> Result<ZG1, Eip4844Error> {
    calc_kzg_proof_with_point(blob, get_evaluation_point(blob, versioned_hash))
}

pub fn calc_kzg_proof_with_point(blob_bytes: &[u8], z: ZFr) -> Result<ZG1, Eip4844Error> {
    let blob = Blob::try_from(blob_bytes).map_err(|_| Eip4844Error::BlobConversion)?;

    let blob_fields = deserialize_blob_rust(&blob).map_err(|_| Eip4844Error::DeserializeBlob)?;
    let (proof, _) = compute_kzg_proof_rust(&blob_fields, &z, &*KZG_SETTINGS)
        .map_err(Eip4844Error::ComputeKzgProof)?;
    Ok(proof)
}

pub fn calc_kzg_proof_commitment(blob_bytes: &[u8]) -> Result<KzgGroup, Eip4844Error> {
    let blob = Blob::try_from(blob_bytes).map_err(|_| Eip4844Error::BlobConversion)?;

    let blob_fields = deserialize_blob_rust(&blob).map_err(|_| Eip4844Error::DeserializeBlob)?;
    Ok(blob_to_kzg_commitment_rust(&blob_fields, &*KZG_SETTINGS)
        .map_err(Eip4844Error::ComputeKzgProof)?
        .to_bytes())
}

pub fn commitment_to_version_hash(commitment: &[u8; 48]) -> B256 {
    let mut hash = Sha256::digest(commitment);
    hash[0] = VERSIONED_HASH_VERSION_KZG;
    B256::new(hash.into())
}

pub fn kzg_proof_to_bytes(proof: &ZG1) -> KzgGroup {
    proof.to_bytes()
}

#[cfg(all(test, not(feature = "zisk")))]
mod test {

    use super::*;
    use alloy_primitives::Bytes;
    use kzg_traits::{
        eip_4844::{verify_kzg_proof_rust, BYTES_PER_FIELD_ELEMENT},
        G1,
    };

    pub fn verify_kzg_proof_evm(
        commitment: &KzgCommitment,
        z: &ZFr,
        y: &ZFr,
        proof: &ZG1,
    ) -> Result<bool, Eip4844Error> {
        verify_kzg_proof_impl(
            *commitment,
            z.to_bytes(),
            y.to_bytes(),
            kzg_proof_to_bytes(proof),
        )
    }

    #[test]
    fn test_kzg_settings_loaded() {
        // Verify the prebuilt KZG settings load successfully and have expected dimensions
        let settings = (*KZG_SETTINGS).clone();
        assert!(
            !settings.g1_values_monomial.is_empty(),
            "g1_values_monomial should not be empty"
        );
        assert!(
            !settings.g2_values_monomial.is_empty(),
            "g2_values_monomial should not be empty"
        );
    }

    #[test]
    fn test_blob_to_kzg_commitment() {
        let blob = Blob::default();
        let commitment = blob_to_kzg_commitment_rust(
            &deserialize_blob_rust(&blob).unwrap(),
            &(*KZG_SETTINGS).clone(),
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
        let kzg_settings = (*KZG_SETTINGS).clone();
        let data: &[u8] = &(0u64..131072).map(|v| (v % 64) as u8).collect::<Vec<u8>>();
        let blob = Blob::try_from(data).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let commitment = calc_kzg_proof_commitment(&blob.0).unwrap();
        let poly = blob_to_polynomial(&blob_fields).unwrap();

        // Random number hash to field
        let x = hash_to_bls_field(&[5; BYTES_PER_FIELD_ELEMENT]);
        let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &kzg_settings).unwrap();
        let proof = calc_kzg_proof_with_point(&blob.0, x).unwrap();

        assert!(verify_kzg_proof_rust(
            &ZG1::from_bytes(&commitment).unwrap(),
            &x,
            &y,
            &proof,
            &kzg_settings,
        )
        .unwrap());
    }

    #[test]
    fn test_verify_kzg_proof_in_precompile() {
        let data: &[u8] = &(0u64..131072).map(|v| (v % 64) as u8).collect::<Vec<u8>>();
        let blob = Blob::try_from(data).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let commitment = calc_kzg_proof_commitment(&blob.0).unwrap();
        let poly = blob_to_polynomial(&blob_fields).unwrap();

        // Random number hash to field
        let x = hash_to_bls_field(&[5; BYTES_PER_FIELD_ELEMENT]);
        let y =
            evaluate_polynomial_in_evaluation_form(&poly, &x, &(*KZG_SETTINGS).clone()).unwrap();
        let proof = calc_kzg_proof_with_point(&blob.0, x).unwrap();

        // Verify a correct proof
        assert!(verify_kzg_proof_evm(&commitment, &x, &y, &proof,).unwrap());

        // Create a proof for a different point
        {
            let x = hash_to_bls_field(&[6; BYTES_PER_FIELD_ELEMENT]);
            let proof = calc_kzg_proof_with_point(&blob.0, x).unwrap();
            assert!(!verify_kzg_proof_evm(&commitment, &x, &y, &proof,).unwrap());
        }

        // Try to prove a different evaluated point
        {
            let y = y.add(&ZFr::one());
            assert!(!verify_kzg_proof_evm(&commitment, &x, &y, &proof,).unwrap());
        }
    }
}
