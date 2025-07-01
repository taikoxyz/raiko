use std::collections::{HashSet, VecDeque};

use raiko_reqpool::{RequestEntity, RequestKey};

/// Queue of requests to be processed
#[derive(Debug)]
pub struct Queue {
    /// High priority pending requests
    high_pending: VecDeque<(RequestKey, RequestEntity)>,
    /// Low priority pending requests
    low_pending: VecDeque<(RequestKey, RequestEntity)>,
    /// Requests that are currently being worked on
    working_in_progress: HashSet<RequestKey>,
    /// Requests that have been pushed to the queue or are in-flight
    queued_keys: HashSet<RequestKey>,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            high_pending: VecDeque::new(),
            low_pending: VecDeque::new(),
            working_in_progress: HashSet::new(),
            queued_keys: HashSet::new(),
        }
    }

    pub fn contains(&self, request_key: &RequestKey) -> bool {
        self.queued_keys.contains(request_key)
    }

    pub fn add_pending(&mut self, request_key: RequestKey, request_entity: RequestEntity) {
        if self.queued_keys.insert(request_key.clone()) {
            let is_high_priority = matches!(request_key, RequestKey::Aggregation(_));
            if is_high_priority {
                self.high_pending.push_back((request_key, request_entity));
            } else {
                self.low_pending.push_back((request_key, request_entity));
            }
        }
    }

    /// Attempts to move a request from either the high or low priority queue into the in-flight set
    /// and starts processing it. High priority requests are processed first.
    pub fn try_next(&mut self) -> Option<(RequestKey, RequestEntity)> {
        // Try high priority queue first
        let (request_key, request_entity) = self
            .high_pending
            .pop_front()
            .or_else(|| self.low_pending.pop_front())?;

        self.working_in_progress.insert(request_key.clone());
        Some((request_key, request_entity))
    }

    pub fn complete(&mut self, request_key: RequestKey) {
        self.working_in_progress.remove(&request_key);
        self.queued_keys.remove(&request_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use raiko_core::interfaces::ProverSpecificOpts;
    use raiko_lib::{input::BlobProofType, primitives::B256, proof_type::ProofType, prover::Proof};
    use raiko_reqpool::{
        AggregationRequestEntity, AggregationRequestKey, SingleProofRequestEntity,
        SingleProofRequestKey,
    };
    use std::collections::HashMap;

    /// Helper function to create a test SingleProof request key - low priority
    fn create_low_priority_request_key(block_number: u64) -> RequestKey {
        let single_proof_key = SingleProofRequestKey::new(
            1u64,
            block_number,
            B256::from([1u8; 32]),
            ProofType::Native,
            "test_prover".to_string(),
        );
        RequestKey::SingleProof(single_proof_key)
    }

    /// Helper function to create a test Aggregation request key - high priority
    fn create_high_priority_request_key(block_numbers: Vec<u64>) -> RequestKey {
        let aggregation_key = AggregationRequestKey::new(ProofType::Native, block_numbers);
        RequestKey::Aggregation(aggregation_key)
    }

    /// Helper function to create a test SingleProof request entity
    fn create_single_proof_request_entity(block_number: u64) -> RequestEntity {
        let single_proof_entity = SingleProofRequestEntity::new(
            block_number,
            5678u64,
            "ethereum".to_string(),
            "ethereum".to_string(),
            B256::from([0u8; 32]),
            Address::ZERO,
            ProofType::Native,
            BlobProofType::ProofOfEquivalence,
            HashMap::new(),
        );
        RequestEntity::SingleProof(single_proof_entity)
    }

    /// Helper function to create a test Aggregation request entity
    fn create_aggregation_request_entity(aggregation_ids: Vec<u64>) -> RequestEntity {
        let aggregation_entity = AggregationRequestEntity::new(
            aggregation_ids,
            vec![Proof::default()],
            ProofType::Native,
            ProverSpecificOpts::default(),
        );
        RequestEntity::Aggregation(aggregation_entity)
    }

    #[test]
    fn test_complex_workflow() {
        let mut queue = Queue::new();

        // Add multiple requests of different priorities
        let low1 = create_low_priority_request_key(1);
        let low2 = create_low_priority_request_key(2);
        let high1 = create_high_priority_request_key(vec![100]);
        let high2 = create_high_priority_request_key(vec![200]);

        let low1_entity = create_single_proof_request_entity(1);
        let low2_entity = create_single_proof_request_entity(2);
        let high1_entity = create_aggregation_request_entity(vec![100]);
        let high2_entity = create_aggregation_request_entity(vec![200]);

        queue.add_pending(low1.clone(), low1_entity);
        queue.add_pending(high1.clone(), high1_entity);
        queue.add_pending(low2.clone(), low2_entity);
        queue.add_pending(high2.clone(), high2_entity);

        // Verify all requests are in queue
        assert_eq!(queue.queued_keys.len(), 4);
        assert_eq!(queue.high_pending.len(), 2);
        assert_eq!(queue.low_pending.len(), 2);

        // Process in priority order
        let (key, _) = queue.try_next().unwrap();
        assert_eq!(key, high1);

        let (key, _) = queue.try_next().unwrap();
        assert_eq!(key, high2);

        let (key, _) = queue.try_next().unwrap();
        assert_eq!(key, low1);

        // Complete one request
        queue.complete(high1.clone());
        assert!(!queue.contains(&high1));
        assert_eq!(queue.working_in_progress.len(), 2); // high2 and low1 still working

        // Get the last request
        let (key, _) = queue.try_next().unwrap();
        assert_eq!(key, low2);

        // Complete remaining requests
        queue.complete(high2);
        queue.complete(low1);
        queue.complete(low2);

        // Queue should be completely empty after all requests are completed
        assert_eq!(queue.queued_keys.len(), 0);
        assert_eq!(queue.working_in_progress.len(), 0);
        assert_eq!(queue.high_pending.len(), 0);
        assert_eq!(queue.low_pending.len(), 0);
    }
}
