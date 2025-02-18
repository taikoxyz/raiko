use raiko_lib::{primitives::BlockHash, proof_type::ProofType};
use std::collections::BTreeMap;

/// Ballot is a proof type selection mechanism using the block hash.
///
/// Note that Ballot is deterministic and does not require any randomness, it uses the deterministic
/// block hash to select the proof type, so that we can replay the proof type selection for the same
/// block hash and get the same result.
#[derive(Debug, Clone, Default)]
pub struct Ballot {
    /// A map of ProofType to their probabilities (between 0 and 1)
    probabilities: BTreeMap<ProofType, f64>,
}

impl Ballot {
    /// Create a new Ballot
    pub fn new(probs: BTreeMap<ProofType, f64>) -> Result<Self, String> {
        let ballot = Self {
            probabilities: probs,
        };
        ballot.validate()?;
        Ok(ballot)
    }

    pub fn probabilities(&self) -> &BTreeMap<ProofType, f64> {
        &self.probabilities
    }

    pub fn validate(&self) -> Result<(), String> {
        // Validate each probability
        for (&proof_type, &prob) in self.probabilities.iter() {
            if 0.0 > prob && 1.0 < prob {
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

    /// Draw proof types based on the block hash.
    pub fn draw(&self, block_hash: &BlockHash) -> Option<ProofType> {
        let block_hash_bytes = block_hash.as_slice();

        // Take the last 16 bytes (least significant) and convert to u128
        let draw_seed = u128::from_le_bytes(block_hash_bytes[16..32].try_into().unwrap());
        // let draw_seed = draw_seed % 10_000;

        let mut cumulative_prob = 0.0;
        for (proof_type, &prob) in self.probabilities.iter() {
            cumulative_prob += prob;
            if draw_seed < (cumulative_prob * u128::MAX as f64).round() as u128 {
                return Some(*proof_type);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draw_deserialize() {
        {
            let serialized = "{\"Sp1\":0.1,\"Risc0\":0.2}";
            let probs: BTreeMap<ProofType, f64> = serde_json::from_str(serialized).unwrap();
            let ballot = Ballot::new(probs).unwrap();
            assert_eq!(ballot.probabilities.len(), 2);
            assert_eq!(ballot.probabilities.get(&ProofType::Sp1), Some(&0.1));
            assert_eq!(ballot.probabilities.get(&ProofType::Risc0), Some(&0.2));
        }
        {
            let serialized = "{}";
            let probs: BTreeMap<ProofType, f64> = serde_json::from_str(serialized).unwrap();
            let ballot = Ballot::new(probs).unwrap();
            assert_eq!(ballot.probabilities.len(), 0);
        }
    }

    #[test]
    fn test_draw_empty_probabilities() {
        let ballot = Ballot::new(BTreeMap::new()).unwrap();
        let block_hash = BlockHash::ZERO;
        let proof_type = ballot.draw(&block_hash);
        assert_eq!(proof_type, None);
    }

    #[test]
    fn test_draw_single_proof_type() {
        let ballot = Ballot::new(BTreeMap::from([(ProofType::Sp1, 1.0)])).unwrap();
        let block_hash = BlockHash::ZERO;
        let proof_type = ballot.draw(&block_hash);
        assert_eq!(proof_type, Some(ProofType::Sp1));
    }

    #[test]
    fn test_draw_multiple_proof_types() {
        let ballot = Ballot::new(BTreeMap::from([
            (ProofType::Sp1, 0.5),
            (ProofType::Risc0, 0.5),
        ]))
        .unwrap();

        let mut proof_type_counts = BTreeMap::new();
        for u in 0..=u8::MAX {
            let block_hash = BlockHash::with_last_byte(u);
            let proof_type = ballot.draw(&block_hash);
            *proof_type_counts.entry(proof_type).or_insert(0) += 1;
        }
        assert_eq!(proof_type_counts.len(), 2);
        assert_eq!(
            proof_type_counts.get(&Some(ProofType::Sp1)),
            Some(&((u8::MAX / 2) + 1))
        );
        assert_eq!(
            proof_type_counts.get(&Some(ProofType::Risc0)),
            Some(&((u8::MAX / 2) + 1))
        );
    }

    #[test]
    fn test_draw_single_50_proof_types() {
        let ballot = Ballot::new(BTreeMap::from([(ProofType::Sp1, 0.5)])).unwrap();

        let mut proof_type_counts = BTreeMap::new();
        for u in 0..=u8::MAX {
            let block_hash = BlockHash::with_last_byte(u);
            let proof_type = ballot.draw(&block_hash);
            *proof_type_counts.entry(proof_type).or_insert(0) += 1;
        }
        assert_eq!(proof_type_counts.len(), 2);
        assert_eq!(
            proof_type_counts.get(&Some(ProofType::Sp1)),
            Some(&((u8::MAX / 2) + 1))
        );
        assert_eq!(proof_type_counts.get(&None), Some(&((u8::MAX / 2) + 1)));
    }
}
