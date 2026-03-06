use crate::{
    backend::Backend, impl_display_using_json_pretty, proof_key_to_hack_request_key, MemoryBackend,
    RedisPoolConfig, RequestEntity, RequestKey, Status, StatusWithContext,
};
use backoff::{exponential::ExponentialBackoff, SystemClock};
use raiko_lib::prover::{IdStore, IdWrite, ProofKey, ProverError, ProverResult};
use raiko_redis_derive::RedisValue;
#[allow(unused_imports)]
use redis::{Client, Commands, RedisResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static LOCAL_BACKEND_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Pool {
    client: Client,
    config: RedisPoolConfig,
    local_backend_namespace: String,
}

impl Pool {
    pub fn add(
        &mut self,
        request_key: RequestKey,
        request_entity: RequestEntity,
        status: StatusWithContext,
    ) -> Result<(), String> {
        tracing::info!("RedisPool.add: {request_key}, {status}");
        let request_entity_and_status = RequestEntityAndStatus {
            entity: request_entity,
            status,
        };
        self.backend_for_key(&request_key)?
            .set_ex(
                request_key,
                request_entity_and_status,
                self.config.redis_ttl,
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn remove(&mut self, request_key: &RequestKey) -> Result<usize, String> {
        tracing::info!("RedisPool.remove: {request_key}");
        if matches!(request_key, RequestKey::ShastaGuestInput(_)) {
            let local_result = self.local_conn().del(request_key).map_err(|e| e.to_string())?;
            let shared_result = self.conn().map_err(|e| e.to_string())?.del(request_key).map_err(|e| e.to_string())?;
            Ok(local_result + shared_result)
        } else {
            let result: usize = self
                .backend_for_key(request_key)?
                .del(request_key)
                .map_err(|e| e.to_string())?;
            Ok(result)
        }
    }

    pub fn get(
        &mut self,
        request_key: &RequestKey,
    ) -> Result<Option<(RequestEntity, StatusWithContext)>, String> {
        if matches!(request_key, RequestKey::ShastaGuestInput(_)) {
            match Self::get_from_backend(self.local_conn(), request_key)? {
                Some(value) => Ok(Some(value)),
                None => {
                    let backend = self.conn().map_err(|e| e.to_string())?;
                    Self::get_from_backend(backend, request_key)
                }
            }
        } else {
            let backend = self.backend_for_key(request_key)?;
            Self::get_from_backend(backend, request_key)
        }
    }

    pub fn get_status(
        &mut self,
        request_key: &RequestKey,
    ) -> Result<Option<StatusWithContext>, String> {
        let result = self.get(request_key).map(|v| v.map(|v| v.1));
        if let Ok(Some(ref status_with_context)) = &result {
            match &status_with_context.status() {
                Status::Failed { error } => {
                    tracing::error!(
                        "RedisPool.get_status: {request_key}, Failed with error: {}",
                        error
                    );
                }
                _ => {
                    tracing::info!(
                        "RedisPool.get_status: {request_key}, {}",
                        status_with_context
                    );
                }
            }
        } else {
            tracing::info!("RedisPool.get_status: {request_key}, None");
        }
        result
    }

    pub fn update_status(
        &mut self,
        request_key: RequestKey,
        status: StatusWithContext,
    ) -> Result<StatusWithContext, String> {
        match self.get(&request_key)? {
            Some((entity, old_status)) => {
                match &status.status() {
                    Status::Failed { error } => {
                        tracing::error!(
                            "RedisPool.update_status: {request_key}, Failed with error: {}",
                            error
                        );
                    }
                    _ => {
                        tracing::info!("RedisPool.update_status: {request_key}, {status}");
                    }
                }
                self.add(request_key, entity, status)?;
                Ok(old_status)
            }
            None => Err("Request not found".to_string()),
        }
    }

    pub fn list(&mut self) -> Result<HashMap<RequestKey, StatusWithContext>, String> {
        let mut result = HashMap::new();
        let mut shared_conn = self.conn().map_err(|e| e.to_string())?;
        let shared_keys: Vec<RequestKey> = shared_conn.keys("*").map_err(|e| e.to_string())?;
        for key in shared_keys {
            if let Ok(Some((_, status))) = self.get(&key) {
                result.insert(key, status);
            }
        }

        let mut local_conn = self.local_conn();
        let local_keys: Vec<RequestKey> = local_conn.keys("*").map_err(|e| e.to_string())?;
        for key in local_keys {
            if let Ok(Some((_, status))) = self.get(&key) {
                result.insert(key, status);
            }
        }

        Ok(result)
    }
}

#[async_trait::async_trait]
impl IdStore for Pool {
    async fn read_id(&mut self, proof_key: ProofKey) -> ProverResult<String> {
        let hack_request_key = proof_key_to_hack_request_key(proof_key);

        tracing::info!("RedisPool.read_id: {hack_request_key}");

        let result: RedisResult<String> = self
            .conn()
            .map_err(|e| e.to_string())?
            .get(&hack_request_key);
        match result {
            Ok(value) => Ok(value.into()),
            Err(e) => Err(ProverError::StoreError(e.to_string())),
        }
    }
}

#[async_trait::async_trait]
impl IdWrite for Pool {
    async fn store_id(&mut self, proof_key: ProofKey, id: String) -> ProverResult<()> {
        let hack_request_key = proof_key_to_hack_request_key(proof_key);

        tracing::info!("RedisPool.store_id: {hack_request_key}, {id}");

        self.conn()
            .map_err(|e| e.to_string())?
            .set_ex(hack_request_key, id, self.config.redis_ttl)
            .map_err(|e| ProverError::StoreError(e.to_string()))?;
        Ok(())
    }

    async fn remove_id(&mut self, proof_key: ProofKey) -> ProverResult<()> {
        let hack_request_key = proof_key_to_hack_request_key(proof_key);

        tracing::info!("RedisPool.remove_id: {hack_request_key}");

        self.conn()
            .map_err(|e| e.to_string())?
            .del(hack_request_key)
            .map_err(|e| ProverError::StoreError(e.to_string()))?;
        Ok(())
    }
}

impl Pool {
    pub fn open(config: RedisPoolConfig) -> Result<Self, redis::RedisError> {
        if config.enable_redis_pool {
            tracing::info!("RedisPool.open using redis: {}", config.redis_url);
        } else {
            tracing::info!("RedisPool.open using memory pool");
        }

        let client = Client::open(config.redis_url.clone())?;
        let local_backend_namespace = format!(
            "{}#local#{}",
            config.redis_url,
            LOCAL_BACKEND_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        Ok(Self {
            client,
            config,
            local_backend_namespace,
        })
    }

    pub fn conn(&mut self) -> Result<Backend, redis::RedisError> {
        self.shared_conn()
    }

    fn local_conn(&self) -> Backend {
        Backend::Memory(MemoryBackend::new(self.local_backend_namespace.clone()))
    }

    fn shared_conn(&mut self) -> Result<Backend, redis::RedisError> {
        if self.config.enable_redis_pool {
            Ok(Backend::Redis(self.redis_conn()?))
        } else {
            Ok(Backend::Memory(MemoryBackend::new(
                self.config.redis_url.clone(),
            )))
        }
    }

    fn backend_for_key(&mut self, request_key: &RequestKey) -> Result<Backend, String> {
        match request_key {
            RequestKey::ShastaGuestInput(_) => Ok(self.local_conn()),
            RequestKey::ShastaProof(_) | RequestKey::ShastaAggregation(_) => {
                self.shared_conn().map_err(|e| e.to_string())
            }
            _ => self.shared_conn().map_err(|e| e.to_string()),
        }
    }

    fn get_from_backend(
        mut backend: Backend,
        request_key: &RequestKey,
    ) -> Result<Option<(RequestEntity, StatusWithContext)>, String> {
        let result: RedisResult<RequestEntityAndStatus> = backend.get(request_key);
        match result {
            Ok(value) => Ok(Some(value.into())),
            Err(e) if e.kind() == redis::ErrorKind::TypeError => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn redis_conn(&mut self) -> Result<redis::Connection, redis::RedisError> {
        let backoff: ExponentialBackoff<SystemClock> = ExponentialBackoff {
            initial_interval: Duration::from_secs(10),
            max_interval: Duration::from_secs(60),
            max_elapsed_time: Some(Duration::from_secs(300)),
            ..Default::default()
        };

        backoff::retry(backoff, || match self.client.get_connection() {
            Ok(conn) => Ok(conn),
            Err(e) => {
                tracing::error!(
                    "RedisPool.get_connection: failed to connect to redis: {e:?}, retrying..."
                );

                self.client = redis::Client::open(self.config.redis_url.clone())?;
                Err(backoff::Error::Transient {
                    err: e,
                    retry_after: None,
                })
            }
        })
        .map_err(|e| match e {
            backoff::Error::Transient {
                err,
                retry_after: _,
            }
            | backoff::Error::Permanent(err) => err,
        })
    }
}

/// A internal wrapper for request entity and status, used for redis serialization
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, RedisValue)]
struct RequestEntityAndStatus {
    entity: RequestEntity,
    status: StatusWithContext,
}

impl From<(RequestEntity, StatusWithContext)> for RequestEntityAndStatus {
    fn from(value: (RequestEntity, StatusWithContext)) -> Self {
        Self {
            entity: value.0,
            status: value.1,
        }
    }
}

impl From<RequestEntityAndStatus> for (RequestEntity, StatusWithContext) {
    fn from(value: RequestEntityAndStatus) -> Self {
        (value.entity, value.status)
    }
}

impl_display_using_json_pretty!(RequestEntityAndStatus);

#[cfg(test)]
mod tests {
    use crate::{
        AggregationRequestEntity, AggregationRequestKey, RequestEntity, ShastaInputRequestEntity,
        ShastaInputRequestKey, ShastaProofRequestEntity, ShastaProofRequestKey,
        SingleProofRequestEntity, SingleProofRequestKey,
    };

    use super::*;
    use alloy_primitives::Address;
    use raiko_core::interfaces::ProverSpecificOpts;
    use raiko_lib::{input::BlobProofType, primitives::B256, proof_type::ProofType};
    use std::collections::HashMap;

    fn shared_memory_pool(id: &str) -> Pool {
        Pool::open(RedisPoolConfig {
            enable_redis_pool: false,
            redis_url: format!("redis://{id}:6379"),
            redis_ttl: 3600,
        })
        .expect("pool opens")
    }

    fn shasta_input_key() -> RequestKey {
        RequestKey::ShastaGuestInput(ShastaInputRequestKey::new(
            1234,
            "ethereum".to_string(),
            "taiko".to_string(),
        ))
    }

    fn shasta_input_entity() -> RequestEntity {
        RequestEntity::ShastaGuestInput(ShastaInputRequestEntity::new(
            1234,
            5678,
            "taiko".to_string(),
            "ethereum".to_string(),
            Address::ZERO,
            BlobProofType::ProofOfEquivalence,
            vec![1, 2, 3],
            None,
            42,
        ))
    }

    fn shasta_proof_key() -> RequestKey {
        RequestKey::ShastaProof(ShastaProofRequestKey::new_with_input_key(
            ShastaInputRequestKey::new(1234, "ethereum".to_string(), "taiko".to_string()),
            ProofType::Native,
            Address::ZERO.to_string(),
        ))
    }

    fn shasta_proof_entity() -> RequestEntity {
        RequestEntity::ShastaProof(ShastaProofRequestEntity::new_with_guest_input_entity(
            ShastaInputRequestEntity::new(
                1234,
                5678,
                "taiko".to_string(),
                "ethereum".to_string(),
                Address::ZERO,
                BlobProofType::ProofOfEquivalence,
                vec![1, 2, 3],
                None,
                42,
            ),
            ProofType::Native,
            HashMap::new(),
        ))
    }

    fn shasta_aggregation_key() -> RequestKey {
        RequestKey::ShastaAggregation(AggregationRequestKey::new(ProofType::Native, vec![1234]))
    }

    fn shasta_aggregation_entity() -> RequestEntity {
        RequestEntity::ShastaAggregation(AggregationRequestEntity::new(
            vec![1234],
            vec![],
            ProofType::Native,
            ProverSpecificOpts::default(),
        ))
    }

    #[ignore]
    #[test]
    fn test_redis_pool_list() {
        // Create Redis pool
        let config = RedisPoolConfig {
            enable_redis_pool: true,
            redis_url: "redis://127.0.0.1:6379".to_string(),
            redis_ttl: 3600,
        };
        let mut pool = Pool::open(config).map_err(|e| e.to_string()).unwrap();

        // Create valid request key and entity
        let request_key = RequestKey::SingleProof(SingleProofRequestKey::new(
            1,
            1234,
            B256::ZERO,
            ProofType::Native,
            "0x1234567890123456789012345678901234567890".to_string(),
        ));

        let request_entity = RequestEntity::SingleProof(SingleProofRequestEntity::new(
            1234,
            5678,
            "sepolia".to_string(),
            "sepolia".to_string(),
            B256::ZERO,
            Address::ZERO,
            ProofType::Native,
            BlobProofType::ProofOfEquivalence,
            HashMap::new(),
        ));

        let status = StatusWithContext::new_registered();

        // Add valid request
        pool.add(request_key.clone(), request_entity, status)
            .unwrap();

        // Try to add invalid data directly to Redis
        let mut conn = pool.conn().unwrap();
        conn.set_ex::<String, String>("invalid_key".to_string(), "invalid_value".to_string(), 3600)
            .unwrap();

        // List should only return valid entries
        let result = pool.list().unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&request_key));
    }

    #[test]
    fn test_shasta_guest_input_is_local_only_for_degraded_shared_backend() {
        let mut pool1 = shared_memory_pool("test_shasta_guest_input_is_local_only");
        let mut pool2 = shared_memory_pool("test_shasta_guest_input_is_local_only");
        let request_key = shasta_input_key();
        let request_entity = shasta_input_entity();
        let status = StatusWithContext::new_registered();

        pool1
            .add(request_key.clone(), request_entity.clone(), status.clone())
            .unwrap();

        assert_eq!(
            pool1.get(&request_key).unwrap(),
            Some((request_entity, status.clone()))
        );
        assert_eq!(pool2.get(&request_key).unwrap(), None);

        pool1
            .update_status(request_key.clone(), Status::WorkInProgress.into())
            .unwrap();
        assert_eq!(pool2.get(&request_key).unwrap(), None);
    }

    #[test]
    fn test_shasta_proof_is_shared_for_degraded_shared_backend() {
        let mut pool1 = shared_memory_pool("test_shasta_proof_is_shared");
        let mut pool2 = shared_memory_pool("test_shasta_proof_is_shared");
        let request_key = shasta_proof_key();
        let request_entity = shasta_proof_entity();
        let status = StatusWithContext::new_registered();

        pool1
            .add(request_key.clone(), request_entity.clone(), status.clone())
            .unwrap();

        assert_eq!(
            pool2.get(&request_key).unwrap(),
            Some((request_entity, status))
        );

        let old_status = pool2
            .update_status(request_key.clone(), Status::WorkInProgress.into())
            .unwrap();
        assert_eq!(old_status.status(), &Status::Registered);

        assert!(matches!(
            pool1.get_status(&request_key).unwrap(),
            Some(updated) if matches!(updated.status(), Status::WorkInProgress)
        ));
    }

    #[test]
    fn test_shasta_aggregation_is_shared_for_degraded_shared_backend() {
        let mut pool1 = shared_memory_pool("test_shasta_aggregation_is_shared");
        let mut pool2 = shared_memory_pool("test_shasta_aggregation_is_shared");
        let request_key = shasta_aggregation_key();
        let request_entity = shasta_aggregation_entity();
        let status = StatusWithContext::new_registered();

        pool1
            .add(request_key.clone(), request_entity.clone(), status.clone())
            .unwrap();

        assert_eq!(pool2.get(&request_key).unwrap(), Some((request_entity, status)));

        assert_eq!(pool2.remove(&request_key).unwrap(), 1);
        assert_eq!(pool1.get(&request_key).unwrap(), None);
    }

    #[test]
    fn test_non_shasta_keys_remain_shared_for_degraded_shared_backend() {
        let mut pool1 = shared_memory_pool("test_non_shasta_keys_remain_shared");
        let mut pool2 = shared_memory_pool("test_non_shasta_keys_remain_shared");
        let request_key = RequestKey::SingleProof(SingleProofRequestKey::new(
            1,
            1234,
            B256::ZERO,
            ProofType::Native,
            Address::ZERO.to_string(),
        ));
        let request_entity = RequestEntity::SingleProof(SingleProofRequestEntity::new(
            1234,
            5678,
            "sepolia".to_string(),
            "sepolia".to_string(),
            B256::ZERO,
            Address::ZERO,
            ProofType::Native,
            BlobProofType::ProofOfEquivalence,
            HashMap::new(),
        ));
        let status = StatusWithContext::new_registered();

        pool1
            .add(request_key.clone(), request_entity.clone(), status.clone())
            .unwrap();

        assert_eq!(pool2.get(&request_key).unwrap(), Some((request_entity, status)));

        assert_eq!(pool2.remove(&request_key).unwrap(), 1);
        assert_eq!(pool1.get(&request_key).unwrap(), None);
    }

    #[test]
    fn test_shasta_guest_input_reads_legacy_shared_entries_for_compatibility() {
        let mut pool = shared_memory_pool("test_shasta_guest_input_reads_legacy_shared_entries");
        let request_key = shasta_input_key();
        let request_entity = shasta_input_entity();
        let status = StatusWithContext::new_registered();
        let legacy_value = RequestEntityAndStatus {
            entity: request_entity.clone(),
            status: status.clone(),
        };

        pool.conn()
            .unwrap()
            .set_ex(request_key.clone(), legacy_value, 3600)
            .unwrap();

        assert_eq!(pool.get(&request_key).unwrap(), Some((request_entity, status)));
        assert!(pool.list().unwrap().contains_key(&request_key));
        assert_eq!(pool.remove(&request_key).unwrap(), 1);
        assert_eq!(pool.get(&request_key).unwrap(), None);
    }
}
