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
use std::{collections::HashMap, time::Duration};

#[derive(Debug, Clone)]
pub struct Pool {
    client: Client,
    config: RedisPoolConfig,
}

impl Pool {
    fn redis_mirror_enabled(&self) -> bool {
        self.config.enable_redis_pool
    }

    /// Shasta guest input stays memory-only; proof and aggregation rows are mirrored to Redis when enabled.
    fn mirrors_shasta_proof_or_agg_to_redis(key: &RequestKey) -> bool {
        matches!(
            key,
            RequestKey::ShastaProof(_) | RequestKey::ShastaAggregation(_)
        )
    }

    fn redis_mirror_set_request(
        &mut self,
        request_key: &RequestKey,
        value: &RequestEntityAndStatus,
    ) {
        match self.redis_conn() {
            Ok(mut conn) => {
                if let Err(e) = conn.set_ex::<RequestKey, RequestEntityAndStatus, ()>(
                    request_key.clone(),
                    value.clone(),
                    self.config.redis_ttl,
                ) {
                    tracing::warn!(
                        %request_key,
                        error = %e,
                        "RedisPool: mirror write failed (memory already updated)"
                    );
                }
            }
            Err(e) => tracing::warn!(
                error = %e,
                "RedisPool: mirror write skipped (no Redis connection)"
            ),
        }
    }

    fn redis_mirror_get_request(
        &mut self,
        request_key: &RequestKey,
    ) -> Result<Option<RequestEntityAndStatus>, String> {
        let mut conn = match self.redis_conn() {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    "RedisPool: mirror get skipped (no Redis connection)"
                );
                return Ok(None);
            }
        };
        let result: RedisResult<RequestEntityAndStatus> = conn.get(request_key);
        match result {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == redis::ErrorKind::TypeError => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn redis_mirror_del_request(&mut self, request_key: &RequestKey) {
        if let Ok(mut conn) = self.redis_conn() {
            let _: RedisResult<usize> = conn.del(request_key.clone());
        }
    }

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
        self.conn()
            .map_err(|e| e.to_string())?
            .set_ex(
                request_key.clone(),
                request_entity_and_status.clone(),
                self.config.redis_ttl,
            )
            .map_err(|e| e.to_string())?;
        if self.redis_mirror_enabled() && Self::mirrors_shasta_proof_or_agg_to_redis(&request_key) {
            self.redis_mirror_set_request(&request_key, &request_entity_and_status);
        }
        Ok(())
    }

    pub fn remove(&mut self, request_key: &RequestKey) -> Result<usize, String> {
        tracing::info!("RedisPool.remove: {request_key}");
        let result: usize = self
            .conn()
            .map_err(|e| e.to_string())?
            .del(request_key)
            .map_err(|e| e.to_string())?;
        if self.redis_mirror_enabled() && Self::mirrors_shasta_proof_or_agg_to_redis(request_key) {
            self.redis_mirror_del_request(request_key);
        }
        Ok(result)
    }

    pub fn get(
        &mut self,
        request_key: &RequestKey,
    ) -> Result<Option<(RequestEntity, StatusWithContext)>, String> {
        let result: RedisResult<RequestEntityAndStatus> =
            self.conn().map_err(|e| e.to_string())?.get(request_key);
        match result {
            Ok(value) => Ok(Some(value.into())),
            Err(e) if e.kind() == redis::ErrorKind::TypeError => {
                if self.redis_mirror_enabled()
                    && Self::mirrors_shasta_proof_or_agg_to_redis(request_key)
                {
                    self.redis_mirror_get_request(request_key)
                        .map(|opt| opt.map(Into::into))
                } else {
                    Ok(None)
                }
            }
            Err(e) => Err(e.to_string()),
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
        let mut conn = self.conn().map_err(|e| e.to_string())?;
        let keys: Vec<RequestKey> = conn.keys("*").map_err(|e| e.to_string())?;

        let mut result = HashMap::new();
        for key in keys {
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
            tracing::info!(
                "RedisPool.open: memory primary + Redis mirror for Shasta proof/aggregation ({})",
                config.redis_url
            );
        } else {
            tracing::info!("RedisPool.open using memory pool only");
        }

        let client = Client::open(config.redis_url.clone())?;
        Ok(Self { client, config })
    }

    /// Primary store is always the in-memory LRU; Redis is only used when mirroring selected keys.
    pub fn conn(&mut self) -> Result<Backend, redis::RedisError> {
        Ok(Backend::Memory(MemoryBackend::new(
            self.config.redis_url.clone(),
        )))
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
    use crate::{SingleProofRequestEntity, SingleProofRequestKey};

    use super::*;
    use alloy_primitives::Address;
    use raiko_lib::{input::BlobProofType, primitives::B256, proof_type::ProofType};
    use std::collections::HashMap;

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
}
