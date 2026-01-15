use std::{
    collections::HashMap,
    // ops::DerefMut,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use raiko_ballot::Ballot;
use raiko_core::interfaces::ProofRequestOpt;
use raiko_lib::{
    consts::{ChainSpec, SupportedChainSpecs},
    proof_type::ProofType,
};
use raiko_reqpool::{Pool, RequestEntity, RequestKey, Status, StatusWithContext};
use reth_primitives::BlockHash;
use tokio::sync::{Mutex, Notify};

use crate::queue::Queue;

/// Actor is the main interface interacting with the backend and the pool.
#[derive(Debug, Clone)]
pub struct Actor {
    default_request_config: ProofRequestOpt,
    chain_specs: SupportedChainSpecs,
    is_paused: Arc<AtomicBool>,

    // TODO: Remove Mutex. currently, in order to pass `&mut Pool`, we need to use Arc<Mutex<Pool>>.
    pool: Arc<Mutex<Pool>>,
    // In order to support dynamic config via HTTP, we need to use Arc<Mutex<Ballot>>.
    ballot: Arc<Mutex<Ballot>>,
    queue: Arc<Mutex<Queue>>,
    notify: Arc<Notify>,
}

impl Actor {
    pub fn new(
        pool: Pool,
        ballot: Ballot,
        default_request_config: ProofRequestOpt,
        chain_specs: SupportedChainSpecs,
        queue: Arc<Mutex<Queue>>,
        notify: Arc<Notify>,
    ) -> Self {
        Self {
            default_request_config,
            chain_specs,
            is_paused: Arc::new(AtomicBool::new(false)),
            ballot: Arc::new(Mutex::new(ballot)),
            pool: Arc::new(Mutex::new(pool)),
            queue,
            notify,
        }
    }

    /// Return the default request config.
    pub fn default_request_config(&self) -> &ProofRequestOpt {
        &self.default_request_config
    }

    /// Return the chain specs.
    pub fn chain_specs(&self) -> &SupportedChainSpecs {
        &self.chain_specs
    }

    pub fn get_chain_spec(&self, network: &str) -> Result<ChainSpec, String> {
        self.chain_specs
            .get_chain_spec(network)
            .ok_or_else(|| format!("unsupported network: {}", network))
    }

    /// Check if the system is paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    /// Get the status of the request from the pool.
    pub async fn pool_get_status(
        &self,
        request_key: &RequestKey,
    ) -> Result<Option<StatusWithContext>, String> {
        self.pool.lock().await.get_status(request_key)
    }

    pub async fn pool_update_status(
        &self,
        request_key: RequestKey,
        status: StatusWithContext,
    ) -> Result<(), String> {
        self.pool
            .lock()
            .await
            .update_status(request_key, status)
            .map(|_| ())
    }

    pub async fn pool_add_new(
        &self,
        request_key: RequestKey,
        request_entity: RequestEntity,
        status: StatusWithContext,
    ) -> Result<(), String> {
        self.pool
            .lock()
            .await
            .add(request_key, request_entity, status)
    }

    pub async fn pool_list_status(&self) -> Result<HashMap<RequestKey, StatusWithContext>, String> {
        self.pool.lock().await.list()
    }

    pub async fn pool_remove_request(&self, request_key: &RequestKey) -> Result<usize, String> {
        self.pool.lock().await.remove(request_key)
    }

    /// Send an action to the backend and wait for the response.
    pub async fn act(
        &self,
        request_key: RequestKey,
        request_entity: RequestEntity,
        start_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<StatusWithContext, String> {
        let pool_status_opt = self.pool_get_status(&request_key).await?;

        // Return successful status if the request is already succeeded
        if matches!(
            pool_status_opt.as_ref().map(|s| s.status()),
            Some(Status::Success { .. })
        ) {
            return Ok(pool_status_opt.unwrap());
        }

        // Mark the request as registered in the pool
        let status = StatusWithContext::new(Status::Registered, start_time);
        if pool_status_opt.is_none() {
            self.pool_add_new(request_key.clone(), request_entity.clone(), status.clone())
                .await?;
        } else {
            self.pool_update_status(request_key.clone(), status.clone())
                .await?;
        }

        // Push the request into the queue and notify to start the action
        let mut queue = self.queue.lock().await;
        if !queue.contains(&request_key) {
            match queue.add_pending(request_key.clone(), request_entity) {
                Ok(()) => {
                    self.notify.notify_one();
                }
                Err(error_msg) => {
                    // If queue is at capacity, update the status to Failed
                    let failed_status =
                        StatusWithContext::new(Status::Failed { error: error_msg }, start_time);
                    self.pool_update_status(request_key.clone(), failed_status.clone())
                        .await?;
                    return Ok(failed_status);
                }
            }
        }

        return Ok(status);
    }

    pub async fn pause(&self) -> Result<(), String> {
        self.is_paused.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub async fn get_ballot(&self) -> Ballot {
        self.ballot.lock().await.clone()
    }

    pub async fn set_ballot(&self, new_ballot: Ballot) {
        let mut ballot = self.ballot.lock().await;
        *ballot = new_ballot;
    }
    pub async fn is_ballot_disabled(&self) -> bool {
        self.ballot.lock().await.probabilities().is_empty()
    }

    /// Draw proof types based on the block hash.
    pub async fn draw(&self, block_hash: &BlockHash) -> Option<ProofType> {
        self.ballot.lock().await.draw_with_poisson(block_hash)
    }

    pub async fn queue_remove(&self, key: &RequestKey) {
        let mut queue = self.queue.lock().await;
        queue.complete(key.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use raiko_ballot::Ballot;
    use raiko_core::interfaces::{ProofRequestOpt, ShastaProposalCheckpoint};
    use raiko_lib::input::BlobProofType;
    use raiko_lib::{consts::SupportedChainSpecs, proof_type::ProofType};
    use raiko_reqpool::{
        Pool, RedisPoolConfig, RequestEntity, RequestKey, ShastaInputRequestKey,
        ShastaProofRequestEntity, ShastaProofRequestKey, SingleProofRequestEntity,
        SingleProofRequestKey, Status,
    };
    use reth_primitives::{BlockHash, B256};
    use std::collections::{BTreeMap, HashMap};
    use tokio::sync::{Mutex, Notify};

    /// Helper function to create a test Actor with a unique ID
    fn create_test_actor_with_id(test_id: &str) -> Actor {
        let config = RedisPoolConfig {
            enable_redis_pool: false,
            redis_url: format!("redis://test_{}:6379", test_id), // Unique URL for isolation
            redis_ttl: 3600,
        };
        let pool = Pool::open(config).unwrap();
        let ballot = Ballot::new(BTreeMap::new()).unwrap();
        let default_request_config = ProofRequestOpt::default();
        let chain_specs = SupportedChainSpecs::default();
        let queue = Arc::new(Mutex::new(Queue::new(1000)));
        let notify = Arc::new(Notify::new());

        Actor::new(
            pool,
            ballot,
            default_request_config,
            chain_specs,
            queue,
            notify,
        )
    }

    /// Helper function to create a test request key
    fn create_test_request_key() -> RequestKey {
        let single_proof_key = SingleProofRequestKey::new(
            1u64,
            1234u64,
            B256::from([1u8; 32]),
            ProofType::Native,
            "test_prover".to_string(),
        );
        RequestKey::SingleProof(single_proof_key)
    }

    /// Helper function to create a test request entity
    fn create_test_request_entity() -> RequestEntity {
        let single_proof_entity = SingleProofRequestEntity::new(
            1234u64,
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

    /// Helper function to create a test request key
    fn create_test_request_shasta_key() -> RequestKey {
        let shasta_input_key =
            ShastaInputRequestKey::new(1u64, "ethereum".to_string(), "ethereum".to_string());
        let actual_prover_address = "test_actual_prover".to_string();
        let shasta_proof_key = ShastaProofRequestKey::new_with_input_key(
            shasta_input_key,
            ProofType::Native,
            actual_prover_address,
        );
        RequestKey::ShastaProof(shasta_proof_key)
    }

    /// Helper function to create a test request entity
    fn create_test_request_shasta_entity() -> RequestEntity {
        let shasta_proof_entity = ShastaProofRequestEntity::new(
            1234u64,
            5678u64,
            "ethereum".to_string(),
            "ethereum".to_string(),
            Address::ZERO,
            ProofType::Native,
            BlobProofType::ProofOfEquivalence,
            vec![1234u64],
            HashMap::new(),
            Some(ShastaProposalCheckpoint {
                block_number: 1234u64,
                block_hash: B256::from([0u8; 32]),
                state_root: B256::from([0u8; 32]),
            }),
            0,
        );
        RequestEntity::ShastaProof(shasta_proof_entity)
    }

    #[tokio::test]
    async fn test_act_with_existing_request() {
        // Initialize logger for test output. This is safe to call multiple times in tests.
        let _ = env_logger::builder().is_test(true).try_init();
        let actor = create_test_actor_with_id("test_act_with_existing_request");
        let request_key = create_test_request_shasta_key();
        let request_entity = create_test_request_shasta_entity();
        let start_time = chrono::Utc::now();

        let mut pool = actor.pool.lock().await;
        pool.add(
            request_key.clone(),
            request_entity.clone(),
            StatusWithContext::new(Status::Registered, start_time),
        )
        .unwrap();

        let ser_entity = serde_json::to_string(&request_entity).unwrap();
        let de_entity = serde_json::from_str(&ser_entity).unwrap();
        assert_eq!(request_entity, de_entity);
    }

    #[tokio::test]
    async fn test_act_with_new_request() {
        let actor = create_test_actor_with_id("test_act_with_new_request");
        let request_key = create_test_request_key();
        let request_entity = create_test_request_entity();
        let start_time = chrono::Utc::now();

        // Test acting on a new request
        let result = actor
            .act(request_key.clone(), request_entity.clone(), start_time)
            .await
            .unwrap();

        assert!(matches!(result.status(), Status::Registered));

        // Verify request was added to pool
        let pool_status = actor.pool_get_status(&request_key).await.unwrap();
        assert!(pool_status.is_some());
        assert!(matches!(pool_status.unwrap().status(), Status::Registered));
    }

    #[tokio::test]
    async fn test_ballot_operations() {
        let actor = create_test_actor_with_id("test_ballot_operations");

        // Test getting ballot
        let ballot = actor.get_ballot().await;
        assert!(ballot.probabilities().is_empty()); // Default ballot should be empty

        // Test if ballot is disabled (empty probabilities)
        assert!(actor.is_ballot_disabled().await);

        // Test setting a new ballot
        let new_ballot = Ballot::new(BTreeMap::new()).unwrap();
        actor.set_ballot(new_ballot.clone()).await;

        let retrieved_ballot = actor.get_ballot().await;
        // The ballot should be the same as what we set
        assert_eq!(retrieved_ballot.probabilities(), new_ballot.probabilities());
    }

    #[tokio::test]
    async fn test_multiple_concurrent_operations() {
        let actor = create_test_actor_with_id("test_multiple_concurrent_operations");
        let mut handles = vec![];

        // Spawn multiple concurrent operations
        for i in 0..10 {
            let actor_clone = actor.clone();
            let handle = tokio::spawn(async move {
                let single_proof_key = SingleProofRequestKey::new(
                    1u64,
                    1234u64 + i as u64,
                    B256::from([i as u8; 32]),
                    ProofType::Native,
                    format!("prover_{}", i),
                );
                let request_key = RequestKey::SingleProof(single_proof_key);
                let request_entity = create_test_request_entity();
                let start_time = chrono::Utc::now();

                actor_clone
                    .act(request_key, request_entity, start_time)
                    .await
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
            assert!(matches!(result.unwrap().status(), Status::Registered));
        }

        // Verify all requests were added to the pool
        let all_statuses = actor.pool_list_status().await.unwrap();
        assert_eq!(all_statuses.len(), 10);
    }

    #[tokio::test]
    async fn test_ballot_with_probabilities() {
        let actor = create_test_actor_with_id("test_ballot_with_probabilities");

        // Create a ballot with some probabilities
        let mut ballot_config = BTreeMap::new();
        ballot_config.insert(ProofType::Native, (0.5, 100));
        ballot_config.insert(ProofType::Sp1, (0.3, 100));

        let new_ballot = Ballot::new(ballot_config.clone()).unwrap();
        actor.set_ballot(new_ballot).await;

        // Test that ballot is not disabled
        assert!(!actor.is_ballot_disabled().await);

        // Test drawing with a specific block hash
        let block_hash = BlockHash::from(B256::from([1u8; 32]));
        let _result = actor.draw(&block_hash).await;
    }

    #[tokio::test]
    async fn test_queue_integration() {
        let actor = create_test_actor_with_id("test_queue_integration");
        let request_key = create_test_request_key();
        let request_entity = create_test_request_entity();
        let start_time = chrono::Utc::now();

        let _result = actor
            .act(request_key.clone(), request_entity.clone(), start_time)
            .await
            .unwrap();

        // Verify the request is in the queue
        let queue = actor.queue.lock().await;
        assert!(queue.contains(&request_key));
    }

    #[tokio::test]
    async fn test_failed_request_requeue() {
        let actor = create_test_actor_with_id("test_failed_request_requeue");
        let request_key = create_test_request_key();
        let request_entity = create_test_request_entity();
        let start_time = chrono::Utc::now();

        // First, add the request as failed in the pool
        let failed_status = StatusWithContext::new(
            Status::Failed {
                error: "test fail".to_string(),
            },
            start_time,
        );
        actor
            .pool_add_new(
                request_key.clone(),
                request_entity.clone(),
                failed_status.clone(),
            )
            .await
            .unwrap();

        // The request should not be in the queue yet
        {
            let queue = actor.queue.lock().await;
            assert!(!queue.contains(&request_key));
        }

        // Act on the request - it should be requeued
        let result = actor
            .act(request_key.clone(), request_entity.clone(), start_time)
            .await
            .unwrap();
        assert!(matches!(result.status(), Status::Registered));

        // The request should now be in the queue
        let queue = actor.queue.lock().await;
        assert!(queue.contains(&request_key));

        // The pool status should be updated to Registered
        let pool_status = actor.pool_get_status(&request_key).await.unwrap().unwrap();
        assert!(matches!(pool_status.status(), Status::Registered));
    }
}
