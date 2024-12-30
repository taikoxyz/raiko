use crate::proof_type::ProofType;
use std::env;

const RISC0_IMAGE_ID_ENV: &str = "RAIKO_RISC0_IMAGE_ID";
const SP1_IMAGE_ID_ENV: &str = "RAIKO_SP1_IMAGE_ID";
const RISC0_AGGREGATION_IMAGE_ID_ENV: &str = "RAIKO_RISC0_AGGREGATION_IMAGE_ID";
const SP1_AGGREGATION_IMAGE_ID_ENV: &str = "RAIKO_SP1_AGGREGATION_IMAGE_ID";

/// Get the default image id for a specific proof type.
/// For RISC0 and SP1 provers, it tries to get the image id from environment variables:
/// - RAIKO_RISC0_IMAGE_ID for RISC0
/// - RAIKO_SP1_IMAGE_ID for SP1
pub fn get_proving_image_id(proof_type: &ProofType) -> String {
    debug_assert!(proof_type == &ProofType::Risc0 || proof_type == &ProofType::Sp1);
    match proof_type {
        ProofType::Risc0 => env::var(RISC0_IMAGE_ID_ENV).ok().unwrap_or_default(),
        ProofType::Sp1 => env::var(SP1_IMAGE_ID_ENV).ok().unwrap_or_default(),
        _ => unreachable!(),
    }
}

/// Get the default aggregation image id for a specific proof type.
/// For RISC0 and SP1 provers, it tries to get the image id from environment variables:
/// - RAIKO_RISC0_AGGREGATION_IMAGE_ID for RISC0
/// - RAIKO_SP1_AGGREGATION_IMAGE_ID for SP1
pub fn get_aggregation_image_id(proof_type: &ProofType) -> String {
    debug_assert!(proof_type == &ProofType::Risc0 || proof_type == &ProofType::Sp1);
    match proof_type {
        ProofType::Risc0 => env::var(RISC0_AGGREGATION_IMAGE_ID_ENV)
            .ok()
            .unwrap_or_default(),
        ProofType::Sp1 => env::var(SP1_AGGREGATION_IMAGE_ID_ENV)
            .ok()
            .unwrap_or_default(),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_prover_image_id() {
        // Test RISC0
        env::set_var(RISC0_IMAGE_ID_ENV, "risc0-test-image");
        assert_eq!(get_proving_image_id(&ProofType::Risc0), "risc0-test-image");
        env::remove_var(RISC0_IMAGE_ID_ENV);
        assert_eq!(get_proving_image_id(&ProofType::Risc0), "");

        // Test SP1
        env::set_var(SP1_IMAGE_ID_ENV, "sp1-test-image");
        assert_eq!(get_proving_image_id(&ProofType::Sp1), "sp1-test-image");
        env::remove_var(SP1_IMAGE_ID_ENV);
        assert_eq!(get_proving_image_id(&ProofType::Sp1), "");

        // Test other proof types
        assert!(std::panic::catch_unwind(|| get_proving_image_id(&ProofType::Native)).is_err());
        assert!(std::panic::catch_unwind(|| get_proving_image_id(&ProofType::Sgx)).is_err());
    }

    #[test]
    fn test_get_aggregation_image_id() {
        // Test RISC0
        env::set_var(RISC0_AGGREGATION_IMAGE_ID_ENV, "risc0-agg-image");
        assert_eq!(
            get_aggregation_image_id(&ProofType::Risc0),
            "risc0-agg-image"
        );
        env::remove_var(RISC0_AGGREGATION_IMAGE_ID_ENV);
        assert_eq!(get_aggregation_image_id(&ProofType::Risc0), "");

        // Test SP1
        env::set_var(SP1_AGGREGATION_IMAGE_ID_ENV, "sp1-agg-image");
        assert_eq!(get_aggregation_image_id(&ProofType::Sp1), "sp1-agg-image");
        env::remove_var(SP1_AGGREGATION_IMAGE_ID_ENV);
        assert_eq!(get_aggregation_image_id(&ProofType::Sp1), "");

        // Test other proof types
        assert!(std::panic::catch_unwind(|| get_aggregation_image_id(&ProofType::Native)).is_err());
        assert!(std::panic::catch_unwind(|| get_aggregation_image_id(&ProofType::Sgx)).is_err());
    }
}
