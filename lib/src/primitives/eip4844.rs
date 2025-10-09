use kzg::kzg_types::{ZFr, ZG1};
use kzg_traits::{
    eip_4844::{
        blob_to_kzg_commitment_rust, blob_to_polynomial, compute_kzg_proof_rust,
        evaluate_polynomial_in_evaluation_form, hash_to_bls_field, Blob,
    },
    Fr, G1,
};
use once_cell::sync::Lazy;
use reth_primitives::B256;
use sha2::{Digest as _, Sha256};

pub use kzg::{eip_4844::deserialize_blob_rust, kzg_proofs::KZGSettings};
pub static KZG_SETTINGS_BIN: &[u8] = include_bytes!("../../kzg_settings/zkcrypto_kzg_settings.bin");

// The KZG settings under the concrete type of kzg backend
// We directly include the serialized struct to avoid conversion cost in guest
// To generate the bytes, run:
//
//     cargo run --bin gen_kzg_settings
pub static KZG_SETTINGS: Lazy<KZGSettings> = Lazy::new(|| {
    bincode::deserialize(KZG_SETTINGS_BIN)
        .expect("failed to load trusted setup, please run `cargo run --bin gen_kzg_settings`")
});

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
}

pub fn get_evaluation_point(blob: &[u8], versioned_hash: &B256) -> ZFr {
    let blob_hash = Sha256::digest(blob);
    let x = Sha256::digest([blob_hash.to_vec(), versioned_hash.to_vec()].concat()).into();
    hash_to_bls_field(&x)
}

pub fn proof_of_equivalence(
    blob: &[u8],
    versioned_hash: &B256,
) -> Result<(KzgField, KzgField), Eip4844Error> {
    let blob_fields = Blob::from_bytes(blob)
        .and_then(|b| deserialize_blob_rust(&b))
        .map_err(|_| Eip4844Error::DeserializeBlob)?;

    let poly = blob_to_polynomial(&blob_fields).unwrap();
    let x = get_evaluation_point(blob, versioned_hash);
    let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &KZG_SETTINGS.clone())
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
    use kzg::bls12_381::*;
    let commitment = G1Affine::from_compressed(&commitment).unwrap();
    let proof = G1Affine::from_compressed(&proof).unwrap();
    let mut x_le = x;
    x_le.reverse();
    let mut y_le = y;
    y_le.reverse();
    let x = Scalar::from_bytes(&x_le).unwrap();
    let y = Scalar::from_bytes(&y_le).unwrap();

    let g2_x = G2Affine::generator() * x;
    let setup_committed_x = G2Affine::from(KZG_SETTINGS.secret_g2[1].proj);
    let x_diff = setup_committed_x - g2_x;

    let g1_y = G1Affine::generator() * y;
    let p_minus_y = commitment - g1_y;

    Ok(pairings_verify(
        p_minus_y.into(),
        G2Affine::generator(),
        proof,
        x_diff.into(),
    ))
}

pub fn calc_kzg_proof(blob: &[u8], versioned_hash: &B256) -> Result<ZG1, Eip4844Error> {
    calc_kzg_proof_with_point(blob, get_evaluation_point(blob, versioned_hash))
}

pub fn calc_kzg_proof_with_point(blob: &[u8], z: ZFr) -> Result<ZG1, Eip4844Error> {
    let blob_fields = Blob::from_bytes(blob)
        .and_then(|b| deserialize_blob_rust(&b))
        .map_err(|_| Eip4844Error::DeserializeBlob)?;
    let (proof, _) = compute_kzg_proof_rust(&blob_fields, &z, &KZG_SETTINGS.clone())
        .map_err(Eip4844Error::ComputeKzgProof)?;
    Ok(proof)
}

pub fn calc_kzg_proof_commitment(blob: &[u8]) -> Result<KzgGroup, Eip4844Error> {
    let blob_fields = Blob::from_bytes(blob)
        .and_then(|b| deserialize_blob_rust(&b))
        .map_err(|_| Eip4844Error::DeserializeBlob)?;
    Ok(
        blob_to_kzg_commitment_rust(&blob_fields, &KZG_SETTINGS.clone())
            .map_err(Eip4844Error::ComputeKzgProof)?
            .to_bytes(),
    )
}

pub fn commitment_to_version_hash(commitment: &[u8; 48]) -> B256 {
    let mut hash = Sha256::digest(commitment);
    hash[0] = VERSIONED_HASH_VERSION_KZG;
    B256::new(hash.into())
}

pub fn kzg_proof_to_bytes(proof: &ZG1) -> KzgGroup {
    proof.to_bytes()
}

#[cfg(test)]
mod test {
    use super::*;
    use kzg_traits::{
        eip_4844::{verify_kzg_proof_rust, BYTES_PER_FIELD_ELEMENT},
        G1,
    };
    use reth_primitives::revm_primitives::kzg::{G1Points, G2Points, G1_POINTS, G2_POINTS};
    use reth_primitives::revm_primitives::Bytes;

    pub fn verify_kzg_proof_evm(
        commitment: &KzgCommitment,
        z: &ZFr,
        y: &ZFr,
        proof: &ZG1,
    ) -> Result<bool, Eip4844Error> {
        // The input is encoded as follows:
        // | versioned_hash |  z  |  y  | commitment | proof |
        // |     32         | 32  | 32  |     48     |   48  |
        let version_hash = commitment_to_version_hash(commitment);
        let mut input = [0u8; 192];
        input[..32].copy_from_slice(&(*version_hash));
        input[32..64].copy_from_slice(&z.to_bytes());
        input[64..96].copy_from_slice(&y.to_bytes());
        input[96..144].copy_from_slice(commitment);
        input[144..192].copy_from_slice(&kzg_proof_to_bytes(proof));

        Ok(reth_primitives::revm_precompile::kzg_point_evaluation::run(
            &Bytes::copy_from_slice(&input),
            u64::MAX,
            &reth_primitives::revm_primitives::env::Env::default(),
        )
        .is_ok())
    }

    #[test]
    fn test_kzg_settings_equivalence() {
        let kzg_settings: KZGSettings = kzg_traits::eip_4844::load_trusted_setup_rust(
            &G1Points::as_ref(G1_POINTS)
                .into_iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>(),
            &G2Points::as_ref(G2_POINTS)
                .into_iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>(),
        )
        .expect("failed to load trusted setup");
        assert_eq!(KZG_SETTINGS.clone().secret_g1, kzg_settings.secret_g1);
        assert_eq!(KZG_SETTINGS.clone().secret_g2, kzg_settings.secret_g2);
    }

    #[test]
    fn test_blob_to_kzg_commitment() {
        let blob = Blob::from_bytes(&[0u8; 131072]).unwrap();
        let commitment = blob_to_kzg_commitment_rust(
            &deserialize_blob_rust(&blob).unwrap(),
            &KZG_SETTINGS.clone(),
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
        let kzg_settings = KZG_SETTINGS.clone();
        let data = (0u64..131072).map(|v| (v % 64) as u8).collect::<Vec<u8>>();
        let blob = Blob::from_bytes(&data).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let commitment = calc_kzg_proof_commitment(&blob.bytes).unwrap();
        let poly = blob_to_polynomial(&blob_fields).unwrap();

        // Random number hash to field
        let x = hash_to_bls_field(&[5; BYTES_PER_FIELD_ELEMENT]);
        let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &kzg_settings).unwrap();
        let proof = calc_kzg_proof_with_point(&blob.bytes, x).unwrap();

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
        let data = (0u64..131072).map(|v| (v % 64) as u8).collect::<Vec<u8>>();
        let blob = Blob::from_bytes(&data).unwrap();
        let blob_fields = deserialize_blob_rust(&blob).unwrap();
        let commitment = calc_kzg_proof_commitment(&blob.bytes).unwrap();
        let poly = blob_to_polynomial(&blob_fields).unwrap();

        // Random number hash to field
        let x = hash_to_bls_field(&[5; BYTES_PER_FIELD_ELEMENT]);
        let y = evaluate_polynomial_in_evaluation_form(&poly, &x, &KZG_SETTINGS.clone()).unwrap();
        let proof = calc_kzg_proof_with_point(&blob.bytes, x).unwrap();

        // Verify a correct proof
        assert!(verify_kzg_proof_evm(&commitment, &x, &y, &proof,).unwrap());

        // Create a proof for a different point
        {
            let x = hash_to_bls_field(&[6; BYTES_PER_FIELD_ELEMENT]);
            let proof = calc_kzg_proof_with_point(&blob.bytes, x).unwrap();
            assert!(!verify_kzg_proof_evm(&commitment, &x, &y, &proof,).unwrap());
        }

        // Try to prove a different evaluated point
        {
            let y = y.add(&ZFr::one());
            assert!(!verify_kzg_proof_evm(&commitment, &x, &y, &proof,).unwrap());
        }
    }
}
