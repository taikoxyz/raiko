use lru::LruCache;
use poisson::PoissionDrawer;
use raiko_lib::{primitives::BlockHash, proof_type::ProofType};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

type BallotDrawKey = BlockHash;
type BallotDrawResult = Option<ProofType>;
/// max 8192 block hash cache for maximum 8192 blocks per day
const CACHE_SIZE: usize = 8192;

mod poisson;

/// Ballot is a proof type selection mechanism using the block hash.
///
/// Note that Ballot is deterministic and does not require any randomness, it uses the deterministic
/// block hash to select the proof type, so that we can replay the proof type selection for the same
/// block hash and get the same result.
#[derive(Debug, Clone)]
pub struct Ballot {
    /// A map of ProofType to their (probability, per_day) tuples
    initial_config: BTreeMap<ProofType, (f64, u64)>,
    /// A map of ProofType to their probabilities (between 0 and 1)
    probabilities: BTreeMap<ProofType, f64>,
    /// A PoissonDrawer to check if the proof type can be drawn
    /// based on the per-day limit and the last draw time
    poisson_drawer: PoissionDrawer,
    /// A cache saves every block hash that has been drawn
    block_hash_cache: Arc<Mutex<LruCache<BallotDrawKey, BallotDrawResult>>>,
}

impl Default for Ballot {
    fn default() -> Self {
        Self::new(BTreeMap::new()).unwrap()
    }
}

impl Ballot {
    /// Create a new Ballot
    pub fn new(ballot_config: BTreeMap<ProofType, (f64, u64)>) -> Result<Self, String> {
        let poisson_check = PoissionDrawer::new(ballot_config.clone());
        let probs = ballot_config
            .iter()
            .map(|(k, v)| (*k, v.0))
            .collect::<BTreeMap<_, _>>();
        let block_hash_cache = Arc::new(Mutex::new(
            LruCache::<BallotDrawKey, BallotDrawResult>::new(
                NonZeroUsize::new(CACHE_SIZE).unwrap(),
            ),
        ));
        let ballot = Self {
            initial_config: ballot_config,
            probabilities: probs,
            poisson_drawer: poisson_check,
            block_hash_cache,
        };
        ballot.validate()?;
        Ok(ballot)
    }

    pub fn probabilities(&self) -> &BTreeMap<ProofType, (f64, u64)> {
        &self.initial_config
    }

    pub fn validate(&self) -> Result<(), String> {
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

    /// Draw proof types based on the block hash.
    fn draw(&self, block_hash: &BlockHash) -> BallotDrawResult {
        let block_hash_bytes = block_hash.as_slice();

        // Take the last 16 bytes (least significant) and convert to u128
        let draw_seed = u128::from_le_bytes(block_hash_bytes[16..32].try_into().unwrap());
        // let draw_seed = draw_seed % 10_000;

        let mut cumulative_prob = 0.0;
        let mut res = None;
        for (proof_type, &prob) in self.probabilities.iter() {
            cumulative_prob += prob;
            if draw_seed < (cumulative_prob * u128::MAX as f64).round() as u128 {
                res = Some(*proof_type);
                break;
            }
        }

        res
    }

    /// Draw proof types based on the block hash.
    pub fn draw_with_poisson(&mut self, block_hash: &BlockHash) -> Option<ProofType> {
        let mut cache = self.block_hash_cache.lock().unwrap();
        // Check cache while holding the lock
        if let Some(res) = cache.get(block_hash).cloned() {
            return res;
        }

        let draw_result = self.draw(block_hash);
        let res = match draw_result {
            Some(ptype) => {
                if self.poisson_drawer.poisson_freq_check(&ptype) {
                    Some(ptype)
                } else {
                    None
                }
            }
            None => None,
        };

        cache.put(*block_hash, res);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_draw_deserialize() {
        {
            let serialized = "{\"Sp1\":[0.1, 100],\"Risc0\":[0.2, 100]}";
            let probs: BTreeMap<ProofType, (f64, u64)> = serde_json::from_str(serialized).unwrap();
            let ballot = Ballot::new(probs).unwrap();
            assert_eq!(ballot.probabilities.len(), 2);
            assert_eq!(ballot.probabilities.get(&ProofType::Sp1), Some(&0.1));
            assert_eq!(ballot.probabilities.get(&ProofType::Risc0), Some(&0.2));
        }
        {
            let serialized = "{}";
            let probs: BTreeMap<ProofType, (f64, u64)> = serde_json::from_str(serialized).unwrap();
            let ballot = Ballot::new(probs).unwrap();
            assert_eq!(ballot.probabilities.len(), 0);
        }
        {
            let serialized = "";
            let probs: BTreeMap<ProofType, (f64, u64)> =
                serde_json::from_str(serialized).unwrap_or_default();
            let ballot = Ballot::new(probs).unwrap();
            assert_eq!(ballot.probabilities.is_empty(), true);
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
        let ballot = Ballot::new(BTreeMap::from([(ProofType::Sp1, (1.0, 100))])).unwrap();
        let block_hash = BlockHash::ZERO;
        let proof_type = ballot.draw(&block_hash);
        assert_eq!(proof_type, Some(ProofType::Sp1));
    }

    #[test]
    fn test_draw_multiple_proof_types() {
        let ballot = Ballot::new(BTreeMap::from([
            (ProofType::Sp1, (0.5, 500)),
            (ProofType::Risc0, (0.5, 500)),
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
        let ballot = Ballot::new(BTreeMap::from([(ProofType::Sp1, (0.5, 100))])).unwrap();
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

    #[test]
    fn test_draw_1_proof_types_with_poisson_checker() {
        let mut ballot = Ballot::new(BTreeMap::from([(ProofType::Sp1, (1.0, 1))])).unwrap();

        let mut proof_type_counts = BTreeMap::new();
        for u in 0..=u8::MAX {
            let block_hash = BlockHash::with_last_byte(u);
            let proof_type = ballot.draw_with_poisson(&block_hash);
            *proof_type_counts.entry(proof_type).or_insert(0) += 1;
        }
        assert_eq!(proof_type_counts.len(), 2);
        assert_eq!(proof_type_counts[&Some(ProofType::Sp1)], 1);
    }

    #[test]
    fn test_draw_256_proof_types_with_poisson_checker() {
        let mut ballot = Ballot::new(BTreeMap::from([(ProofType::Sp1, (1.0, 3600 * 24))])).unwrap();

        let mut proof_type_counts = BTreeMap::new();
        for u in 0..5 {
            let block_hash = BlockHash::with_last_byte(u);
            let proof_type = ballot.draw_with_poisson(&block_hash);
            *proof_type_counts.entry(proof_type).or_insert(0) += 1;
            sleep(std::time::Duration::from_secs(1));
        }
        assert_eq!(proof_type_counts.len(), 1);
        assert_eq!(proof_type_counts[&Some(ProofType::Sp1)], 5);
    }

    #[test]
    fn test_draw_single_50_proof_types_with_0_poisson_check() {
        let ballot = Ballot::new(BTreeMap::from([(ProofType::Sp1, (0.5, 0))])).unwrap();

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
