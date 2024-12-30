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
pub fn get_prover_image_id(proof_type: &ProofType) -> Option<String> {
    match proof_type {
        ProofType::Risc0 => env::var(RISC0_IMAGE_ID_ENV).ok(),
        ProofType::Sp1 => env::var(SP1_IMAGE_ID_ENV).ok(),
        _ => None,
    }
}

/// Get the default aggregation image id for a specific proof type.
/// For RISC0 and SP1 provers, it tries to get the image id from environment variables:
/// - RAIKO_RISC0_AGGREGATION_IMAGE_ID for RISC0
/// - RAIKO_SP1_AGGREGATION_IMAGE_ID for SP1
pub fn get_aggregation_image_id(proof_type: &ProofType) -> Option<String> {
    match proof_type {
        ProofType::Risc0 => env::var(RISC0_AGGREGATION_IMAGE_ID_ENV).ok(),
        ProofType::Sp1 => env::var(SP1_AGGREGATION_IMAGE_ID_ENV).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_prover_image_id() {
        // Test RISC0
        env::set_var(RISC0_IMAGE_ID_ENV, "risc0-test-image");
        assert_eq!(
            get_prover_image_id(&ProofType::Risc0),
            Some("risc0-test-image".to_string())
        );
        env::remove_var(RISC0_IMAGE_ID_ENV);
        assert_eq!(get_prover_image_id(&ProofType::Risc0), None);

        // Test SP1
        env::set_var(SP1_IMAGE_ID_ENV, "sp1-test-image");
        assert_eq!(
            get_prover_image_id(&ProofType::Sp1),
            Some("sp1-test-image".to_string())
        );
        env::remove_var(SP1_IMAGE_ID_ENV);
        assert_eq!(get_prover_image_id(&ProofType::Sp1), None);

        // Test other proof types
        assert_eq!(get_prover_image_id(&ProofType::Native), None);
        assert_eq!(get_prover_image_id(&ProofType::Sgx), None);
    }

    #[test]
    fn test_get_aggregation_image_id() {
        // Test RISC0
        env::set_var(RISC0_AGGREGATION_IMAGE_ID_ENV, "risc0-agg-image");
        assert_eq!(
            get_aggregation_image_id(&ProofType::Risc0),
            Some("risc0-agg-image".to_string())
        );
        env::remove_var(RISC0_AGGREGATION_IMAGE_ID_ENV);
        assert_eq!(get_aggregation_image_id(&ProofType::Risc0), None);

        // Test SP1
        env::set_var(SP1_AGGREGATION_IMAGE_ID_ENV, "sp1-agg-image");
        assert_eq!(
            get_aggregation_image_id(&ProofType::Sp1),
            Some("sp1-agg-image".to_string())
        );
        env::remove_var(SP1_AGGREGATION_IMAGE_ID_ENV);
        assert_eq!(get_aggregation_image_id(&ProofType::Sp1), None);

        // Test other proof types
        assert_eq!(get_aggregation_image_id(&ProofType::Native), None);
        assert_eq!(get_aggregation_image_id(&ProofType::Sgx), None);
    }
}
