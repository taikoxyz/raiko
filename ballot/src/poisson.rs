use chrono::{DateTime, Duration, Utc};
use raiko_lib::proof_type::ProofType;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Possion is a proof type selection mechanism using the jittered uniform Poisson-like scheduler
///
#[derive(Debug, Clone, Default)]
pub struct PoissionDrawer {
    pub per_day_limit: HashMap<ProofType, usize>,
    pub interval_secs: HashMap<ProofType, i64>,
    pub last_draw_time: HashMap<ProofType, DateTime<Utc>>,
}

impl PoissionDrawer {
    /// Construct ProofDrawer from (rate, per_day) tuples per proof type
    pub fn new(config: BTreeMap<ProofType, (f64, u64)>) -> Self {
        let mut per_day_limit = HashMap::new();
        let mut interval_secs = HashMap::new();
        let mut last_draw_time = HashMap::new();

        for (ptype, (_rate, per_day)) in config {
            per_day_limit.insert(ptype.clone(), per_day as usize);
            let interval = if per_day == 0 {
                0
            } else {
                // Convert per_day to seconds and round to the nearest integer
                (24.0 * 3600.0 / per_day as f64).round() as i64
            };
            interval_secs.insert(ptype, interval);
            last_draw_time.insert(ptype, Utc::now() - Duration::seconds(interval));
        }

        tracing::info!(
            "PoissionDrawer limit: {:?}, intervals: {:?}",
            per_day_limit,
            interval_secs
        );

        Self {
            per_day_limit,
            interval_secs,
            last_draw_time,
        }
    }

    fn enabled(&self, proof_type: &ProofType) -> bool {
        self.per_day_limit.get(proof_type).unwrap_or(&0) > &0
    }

    /// Decide whether to trigger a proof for a given type based on last time and now
    pub fn poisson_freq_check(&mut self, proof_type: &ProofType) -> bool {
        if !self.enabled(proof_type) {
            return true;
        }

        let now: DateTime<Utc> = Utc::now();
        let last_time: DateTime<Utc> = self
            .last_draw_time
            .get(proof_type)
            .cloned()
            .unwrap_or_default();
        let delta = now.signed_duration_since(&last_time).num_seconds();
        if delta <= 0 {
            return false;
        }

        let interval = match self.interval_secs.get(proof_type) {
            Some(i) => *i,
            None => return false,
        };

        tracing::debug!(
            "draw: {:?}, last_time: {:?}, now: {:?}, delta: {}, interval: {}",
            proof_type,
            last_time,
            now,
            delta,
            interval
        );

        let draw_result = delta >= interval;
        if draw_result {
            self.last_draw_time.insert(*proof_type, now);
        }
        draw_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[ignore = "test for print"]
    #[test]
    fn test_btree_ser() {
        let mut config = BTreeMap::new();
        config.insert(ProofType::Risc0, (0.5, 100));
        config.insert(ProofType::Sp1, (0.5, 200));

        println!("btree ser: {:?}", serde_json::to_string(&config).ok());
    }

    #[test]
    fn test_default_disabled_poisson_check() {
        let config_str = "{}";
        let config = serde_json::from_str(config_str).unwrap();
        let drawer = PoissionDrawer::new(config);

        assert!(!drawer.enabled(&ProofType::Sp1));
        assert!(!drawer.enabled(&ProofType::Risc0));
    }

    #[test]
    fn test_default_partially_disabled_poisson_check() {
        let config_str = "{\"Sp1\":[0.5,0], \"Risc0\":[0.5,100]}";
        let config = serde_json::from_str(config_str).unwrap();
        let drawer = PoissionDrawer::new(config);

        assert!(!drawer.enabled(&ProofType::Native));
        assert!(!drawer.enabled(&ProofType::Sp1));
        assert!(drawer.enabled(&ProofType::Risc0));
    }

    #[test]
    fn test_should_draw_per_type() {
        let config_str = "{\"Sp1\":[1.0,200],\"Risc0\":[1.0,100]}";
        let config = serde_json::from_str(config_str).unwrap();
        let mut drawer = PoissionDrawer::new(config);

        let result_a = drawer.poisson_freq_check(&ProofType::Sp1);
        let result_b = drawer.poisson_freq_check(&ProofType::Risc0);

        assert!(result_a);
        assert!(result_b);
    }
}
