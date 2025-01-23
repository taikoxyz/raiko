use raiko_lib::proof_type::ProofType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Ballot is a proof type selection mechanism using the block number as the random number.
///
/// Note that Ballot is deterministic and does not require any randomness, it uses the deterministic
/// block number to select the proof type, so that we can replay the proof type selection for the same
/// block number and get the same result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ballot {
    /// The proof type to always use
    always_proof_type: ProofType,

    /// A map of ProofType to their probabilities (between 0 and 1)
    probabilities: BTreeMap<ProofType, f64>,
}

impl Ballot {
    /// Create a new Ballot
    pub fn new(
        always_proof_type: ProofType,
        probs: BTreeMap<ProofType, f64>,
    ) -> Result<Self, String> {
        let ballot = Self {
            always_proof_type,
            probabilities: probs,
        };
        ballot.validate()?;
        Ok(ballot)
    }

    pub fn validate(&self) -> Result<(), String> {
        // Validate the allowed proof types
        let allowed_proof_types = vec![ProofType::Sp1, ProofType::Risc0];
        for (proof_type, _) in self.probabilities.iter() {
            if !allowed_proof_types.contains(proof_type) {
                return Err(format!("Not allowed prob proof type: {proof_type}"));
            }
        }

        // Validate the always proof type
        if !allowed_proof_types.contains(&self.always_proof_type) {
            return Err(format!(
                "Not allowed always proof type: {}",
                self.always_proof_type
            ));
        }

        // Validate each probability
        for (&proof_type, &prob) in self.probabilities.iter() {
            if !(0.0..=1.0).contains(&prob) {
                return Err(format!(
                    "Invalid probability value {} for proof type {:?}, must be between 0 and 1",
                    prob, proof_type
                ));
            }
        }

        // Validate the total probability
        let total_prob: f64 = self.probabilities.values().sum();
        if total_prob > 1.0 {
            return Err(format!(
                "Total probability must be less than or equal to 1.0, but got {total_prob}"
            ));
        }

        Ok(())
    }

    /// Draw proof types based on the block number, returning the always proof type and the proof type
    /// selected by the block number.
    pub fn draw(&self, block_number: u64) -> (ProofType, Option<ProofType>) {
        const PRECISION: u64 = 10_000;
        let remainder = block_number % PRECISION;

        let mut cumulative_prob = 0.0;
        for (proof_type, &prob) in self.probabilities.iter() {
            cumulative_prob += prob;
            if remainder < (cumulative_prob * PRECISION as f64).round() as u64 {
                return (self.always_proof_type, Some(*proof_type));
            }
        }

        (self.always_proof_type, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ballot_new_valid() {
        let mut probs = BTreeMap::new();
        probs.insert(ProofType::Sp1, 0.3);
        probs.insert(ProofType::Risc0, 0.4);

        let ballot = Ballot::new(ProofType::Sp1, probs).unwrap();
        assert_eq!(ballot.always_proof_type, ProofType::Sp1);
        assert_eq!(ballot.probabilities.len(), 2);
        assert_eq!(ballot.probabilities.get(&ProofType::Sp1), Some(&0.3));
        assert_eq!(ballot.probabilities.get(&ProofType::Risc0), Some(&0.4));
    }

    #[test]
    fn test_ballot_new_invalid_probability() {
        let mut probs = BTreeMap::new();
        probs.insert(ProofType::Sp1, 1.5); // Invalid probability > 1.0

        let result = Ballot::new(ProofType::Sp1, probs);
        assert!(result.is_err());
    }

    #[test]
    fn test_ballot_new_total_probability_exceeds_one() {
        let mut probs = BTreeMap::new();
        probs.insert(ProofType::Sp1, 0.6);
        probs.insert(ProofType::Risc0, 0.5);

        let result = Ballot::new(ProofType::Sp1, probs);
        assert!(result.is_err());
    }

    #[test]
    fn test_ballot_draw() {
        let mut probs = BTreeMap::new();
        probs.insert(ProofType::Sp1, 0.3);
        probs.insert(ProofType::Risc0, 0.4);

        let ballot = Ballot::new(ProofType::Sp1, probs).unwrap();

        // Test with block numbers that should hit different ranges
        // Block 0 should fall into Sp1's range (0-3000)
        let (always, selected) = ballot.draw(0);
        assert_eq!(always, ProofType::Sp1);
        assert_eq!(selected, Some(ProofType::Sp1));

        // Block 5000 should fall into Risc0's range (3000-7000)
        let (always, selected) = ballot.draw(5000);
        assert_eq!(always, ProofType::Sp1);
        assert_eq!(selected, Some(ProofType::Risc0));

        // Block 8000 should fall outside both ranges (>7000)
        let (always, selected) = ballot.draw(8000);
        assert_eq!(always, ProofType::Sp1);
        assert_eq!(selected, None);
    }

    #[test]
    fn test_ballot_serialization() {
        let mut probs = BTreeMap::new();
        probs.insert(ProofType::Sp1, 0.3);
        probs.insert(ProofType::Risc0, 0.4);

        let ballot = Ballot::new(ProofType::Sp1, probs).unwrap();

        // Test JSON serialization
        // {"always_proof_type":"Sp1","probabilities":{"Sp1":0.3,"Risc0":0.4}}
        let json = serde_json::to_string(&ballot).unwrap();
        let deserialized: Ballot = serde_json::from_str(&json).unwrap();

        assert_eq!(ballot.always_proof_type, deserialized.always_proof_type);
        assert_eq!(ballot.probabilities, deserialized.probabilities);
    }

    #[test]
    fn test_ballot_default() {
        let ballot = Ballot::default();
        assert_eq!(ballot.always_proof_type, ProofType::default());
        assert!(ballot.probabilities.is_empty());
    }
}
