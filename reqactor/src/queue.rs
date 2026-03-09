use raiko_reqpool::{RequestEntity, RequestKey, RequestStage};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

/// Min-heap item ordered by `(sort_key, stage)`.
///
/// Rust's `BinaryHeap` is a max-heap, so we invert the comparison
/// to pop the lowest sort_key (and lowest stage within ties) first.
#[derive(Debug)]
struct PriorityItem {
    /// The sort key for ordering (e.g., batch_id or proposal_id).
    sort_key: u64,
    /// The stage of the request (GuestInput < Proof < Aggregation).
    stage: RequestStage,
    request_key: RequestKey,
    request_entity: RequestEntity,
}

impl PriorityItem {
    fn new(request_key: RequestKey, request_entity: RequestEntity) -> Self {
        Self {
            sort_key: request_key.batch_sort_key(),
            stage: request_key.stage(),
            request_key,
            request_entity,
        }
    }
}

impl PartialEq for PriorityItem {
    fn eq(&self, other: &Self) -> bool {
        self.sort_key == other.sort_key && self.stage == other.stage
    }
}
impl Eq for PriorityItem {}

impl PartialOrd for PriorityItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .sort_key
            .cmp(&self.sort_key)
            .then_with(|| other.stage.cmp(&self.stage))
    }
}

/// Priority queue for proof requests.
///
/// All requests go into a single [`BinaryHeap`] ordered by:
///   1. ascending `sort_key` (batch_id / proposal_id — lower first)
///   2. ascending `stage` (guest_input → proof → aggregation)
#[derive(Debug)]
pub struct Queue {
    heap: BinaryHeap<PriorityItem>,
    working_in_progress: HashSet<RequestKey>,
    /// All keys currently in the heap or in-progress (used for dedup + capacity).
    queued_keys: HashSet<RequestKey>,
    max_queue_size: usize,
}

impl Queue {
    pub fn new(max_queue_size: usize) -> Self {
        Self {
            heap: BinaryHeap::new(),
            working_in_progress: HashSet::new(),
            queued_keys: HashSet::new(),
            max_queue_size,
        }
    }

    pub fn contains(&self, request_key: &RequestKey) -> bool {
        self.queued_keys.contains(request_key)
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    pub fn is_at_capacity(&self) -> bool {
        self.queued_keys.len() >= self.max_queue_size
    }

    /// Total keys tracked (pending + in-progress).
    pub fn size(&self) -> usize {
        self.queued_keys.len()
    }

    /// Number of items waiting to be dispatched (excludes in-progress).
    pub fn pending_len(&self) -> usize {
        self.heap.len()
    }

    pub fn add_pending(
        &mut self,
        request_key: RequestKey,
        request_entity: RequestEntity,
    ) -> Result<(), String> {
        if self.is_at_capacity() {
            return Err("Reached the maximum queue size, please try again later".to_string());
        }

        if self.queued_keys.insert(request_key.clone()) {
            let item = PriorityItem::new(request_key, request_entity);
            tracing::info!(
                item.sort_key,
                %item.stage,
                kind = %item.request_key,
                "Adding request to priority heap"
            );
            self.heap.push(item);
        }
        Ok(())
    }

    /// Pops the highest-priority pending request and marks it in-progress.
    pub fn try_next(&mut self) -> Option<(RequestKey, RequestEntity)> {
        let PriorityItem {
            request_key,
            request_entity,
            ..
        } = self.heap.pop()?;
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
        AggregationRequestEntity, AggregationRequestKey, BatchGuestInputRequestEntity,
        BatchGuestInputRequestKey, BatchProofRequestEntity, BatchProofRequestKey,
        ShastaInputRequestEntity, ShastaInputRequestKey, ShastaProofRequestEntity,
        ShastaProofRequestKey, SingleProofRequestEntity, SingleProofRequestKey,
    };
    use std::collections::HashMap;

    // ── helpers ──────────────────────────────────────────────────────────

    fn make_single_proof_key(block_number: u64) -> RequestKey {
        RequestKey::SingleProof(SingleProofRequestKey::new(
            1u64,
            block_number,
            B256::from([1u8; 32]),
            ProofType::Native,
            "test_prover".to_string(),
        ))
    }

    fn make_single_proof_entity(block_number: u64) -> RequestEntity {
        RequestEntity::SingleProof(SingleProofRequestEntity::new(
            block_number,
            5678u64,
            "ethereum".to_string(),
            "ethereum".to_string(),
            B256::from([0u8; 32]),
            Address::ZERO,
            ProofType::Native,
            BlobProofType::ProofOfEquivalence,
            HashMap::new(),
        ))
    }

    fn make_aggregation_key(block_numbers: Vec<u64>) -> RequestKey {
        RequestKey::Aggregation(AggregationRequestKey::new(ProofType::Native, block_numbers))
    }

    fn make_aggregation_entity(block_numbers: Vec<u64>) -> RequestEntity {
        RequestEntity::Aggregation(AggregationRequestEntity::new(
            block_numbers,
            vec![Proof::default()],
            ProofType::Native,
            ProverSpecificOpts::default(),
        ))
    }

    fn make_shasta_agg_key(proposal_ids: Vec<u64>) -> RequestKey {
        RequestKey::ShastaAggregation(AggregationRequestKey::new(ProofType::Native, proposal_ids))
    }

    fn make_shasta_agg_entity(proposal_ids: Vec<u64>) -> RequestEntity {
        RequestEntity::ShastaAggregation(AggregationRequestEntity::new(
            proposal_ids,
            vec![Proof::default()],
            ProofType::Native,
            ProverSpecificOpts::default(),
        ))
    }

    fn make_batch_proof_key(batch_id: u64) -> RequestKey {
        RequestKey::BatchProof(BatchProofRequestKey::new(
            1u64,
            batch_id,
            100u64,
            ProofType::Native,
            "prover".to_string(),
        ))
    }

    fn make_batch_proof_entity(batch_id: u64) -> RequestEntity {
        RequestEntity::BatchProof(BatchProofRequestEntity::new(
            batch_id,
            100u64,
            "ethereum".to_string(),
            "ethereum".to_string(),
            B256::from([0u8; 32]),
            Address::ZERO,
            ProofType::Native,
            BlobProofType::ProofOfEquivalence,
            HashMap::new(),
        ))
    }

    fn make_batch_guest_input_key(batch_id: u64) -> RequestKey {
        RequestKey::BatchGuestInput(BatchGuestInputRequestKey::new(1u64, batch_id, 100u64))
    }

    fn make_batch_guest_input_entity(batch_id: u64) -> RequestEntity {
        RequestEntity::BatchGuestInput(BatchGuestInputRequestEntity::new(
            batch_id,
            100u64,
            "ethereum".to_string(),
            "ethereum".to_string(),
            B256::from([0u8; 32]),
            BlobProofType::ProofOfEquivalence,
        ))
    }

    fn make_shasta_guest_input_key(proposal_id: u64) -> RequestKey {
        RequestKey::ShastaGuestInput(ShastaInputRequestKey::new(
            proposal_id,
            "l1".to_string(),
            "l2".to_string(),
        ))
    }

    fn make_shasta_guest_input_entity(proposal_id: u64) -> RequestEntity {
        RequestEntity::ShastaGuestInput(ShastaInputRequestEntity::new(
            proposal_id,
            100u64,
            "l2".to_string(),
            "l1".to_string(),
            Address::ZERO,
            BlobProofType::ProofOfEquivalence,
            vec![1, 2, 3],
            Default::default(),
            0,
        ))
    }

    fn make_shasta_proof_key(proposal_id: u64) -> RequestKey {
        RequestKey::ShastaProof(ShastaProofRequestKey::new_with_input_key_and_image_id(
            ShastaInputRequestKey::new(proposal_id, "l1".to_string(), "l2".to_string()),
            ProofType::Native,
            "prover".to_string(),
            Default::default(),
        ))
    }

    fn make_shasta_proof_entity(proposal_id: u64) -> RequestEntity {
        RequestEntity::ShastaProof(ShastaProofRequestEntity::new_with_guest_input_entity(
            ShastaInputRequestEntity::new(
                proposal_id,
                100u64,
                "l2".to_string(),
                "l1".to_string(),
                Address::ZERO,
                BlobProofType::ProofOfEquivalence,
                vec![1, 2, 3],
                Default::default(),
                0,
            ),
            ProofType::Native,
            HashMap::new().into(),
        ))
    }

    // ── tests ───────────────────────────────────────────────────────────

    #[test]
    fn test_sort_key_ordering() {
        let mut queue = Queue::new(10);

        // SingleProof sort_key = block_number, stage = Proof
        let single1 = make_single_proof_key(1);
        let single5 = make_single_proof_key(5);
        let batch3 = make_batch_proof_key(3);

        queue
            .add_pending(single5.clone(), make_single_proof_entity(5))
            .unwrap();
        queue
            .add_pending(batch3.clone(), make_batch_proof_entity(3))
            .unwrap();
        queue
            .add_pending(single1.clone(), make_single_proof_entity(1))
            .unwrap();

        // All stage=Proof, so pure sort_key ordering: 1, 3, 5
        assert_eq!(queue.try_next().unwrap().0, single1);
        assert_eq!(queue.try_next().unwrap().0, batch3);
        assert_eq!(queue.try_next().unwrap().0, single5);
    }

    /// Within the same sort_key: guest_input → proof → aggregation.
    #[test]
    fn test_stage_ordering_within_same_sort_key() {
        let mut queue = Queue::new(10);

        let agg = make_aggregation_key(vec![5]);
        let proof = make_batch_proof_key(5);
        let guest = make_batch_guest_input_key(5);

        queue
            .add_pending(agg.clone(), make_aggregation_entity(vec![5]))
            .unwrap();
        queue
            .add_pending(proof.clone(), make_batch_proof_entity(5))
            .unwrap();
        queue
            .add_pending(guest.clone(), make_batch_guest_input_entity(5))
            .unwrap();

        assert_eq!(queue.try_next().unwrap().0, guest);
        assert_eq!(queue.try_next().unwrap().0, proof);
        assert_eq!(queue.try_next().unwrap().0, agg);
    }

    /// Lower sort_key wins regardless of stage.
    #[test]
    fn test_sort_key_before_stage() {
        let mut queue = Queue::new(10);

        let agg1 = make_aggregation_key(vec![1]); // sort_key=1, Aggregation
        let guest2 = make_batch_guest_input_key(2); // sort_key=2, GuestInput

        queue
            .add_pending(guest2.clone(), make_batch_guest_input_entity(2))
            .unwrap();
        queue
            .add_pending(agg1.clone(), make_aggregation_entity(vec![1]))
            .unwrap();

        assert_eq!(queue.try_next().unwrap().0, agg1);
        assert_eq!(queue.try_next().unwrap().0, guest2);
    }

    /// Full Shasta pipeline for two proposals:
    /// gi(1) → proof(1) → agg(1,2) → gi(2) → proof(2)
    #[test]
    fn test_shasta_full_flow_ordering() {
        let mut queue = Queue::new(20);

        let gi1 = make_shasta_guest_input_key(1);
        let gi2 = make_shasta_guest_input_key(2);
        let proof1 = make_shasta_proof_key(1);
        let proof2 = make_shasta_proof_key(2);
        let agg = make_shasta_agg_key(vec![1, 2]); // sort_key = 1

        queue
            .add_pending(proof2.clone(), make_shasta_proof_entity(2))
            .unwrap();
        queue
            .add_pending(agg.clone(), make_shasta_agg_entity(vec![1, 2]))
            .unwrap();
        queue
            .add_pending(gi2.clone(), make_shasta_guest_input_entity(2))
            .unwrap();
        queue
            .add_pending(proof1.clone(), make_shasta_proof_entity(1))
            .unwrap();
        queue
            .add_pending(gi1.clone(), make_shasta_guest_input_entity(1))
            .unwrap();

        assert_eq!(queue.try_next().unwrap().0, gi1);
        assert_eq!(queue.try_next().unwrap().0, proof1);
        assert_eq!(queue.try_next().unwrap().0, agg);
        assert_eq!(queue.try_next().unwrap().0, gi2);
        assert_eq!(queue.try_next().unwrap().0, proof2);
        assert!(queue.try_next().is_none());
    }

    /// Batch and Shasta items are sorted together in one heap.
    #[test]
    fn test_mixed_batch_and_shasta_sorted() {
        let mut queue = Queue::new(20);

        let shasta_gi3 = make_shasta_guest_input_key(3);
        let batch_gi1 = make_batch_guest_input_key(1);
        let shasta_proof3 = make_shasta_proof_key(3);
        let batch_proof1 = make_batch_proof_key(1);

        queue
            .add_pending(shasta_proof3.clone(), make_shasta_proof_entity(3))
            .unwrap();
        queue
            .add_pending(batch_proof1.clone(), make_batch_proof_entity(1))
            .unwrap();
        queue
            .add_pending(shasta_gi3.clone(), make_shasta_guest_input_entity(3))
            .unwrap();
        queue
            .add_pending(batch_gi1.clone(), make_batch_guest_input_entity(1))
            .unwrap();

        assert_eq!(queue.try_next().unwrap().0, batch_gi1);
        assert_eq!(queue.try_next().unwrap().0, batch_proof1);
        assert_eq!(queue.try_next().unwrap().0, shasta_gi3);
        assert_eq!(queue.try_next().unwrap().0, shasta_proof3);
    }

    #[test]
    fn test_complete_removes_from_tracking() {
        let mut queue = Queue::new(10);

        let key = make_batch_proof_key(1);
        queue
            .add_pending(key.clone(), make_batch_proof_entity(1))
            .unwrap();

        assert_eq!(queue.size(), 1);
        assert_eq!(queue.pending_len(), 1);

        let (k, _) = queue.try_next().unwrap();

        // In-progress: size still 1, pending now 0
        assert_eq!(queue.size(), 1);
        assert_eq!(queue.pending_len(), 0);

        queue.complete(k);
        assert_eq!(queue.size(), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_queue_limit() {
        let mut queue = Queue::new(2);

        for i in 0..2 {
            let key = make_single_proof_key(i as u64);
            let entity = make_single_proof_entity(i as u64);
            assert!(queue.add_pending(key, entity).is_ok());
        }

        assert_eq!(queue.size(), 2);
        assert!(queue.is_at_capacity());

        let result = queue.add_pending(make_single_proof_key(3), make_single_proof_entity(3));
        assert!(result.is_err());
        assert_eq!(queue.size(), 2);
    }
}
