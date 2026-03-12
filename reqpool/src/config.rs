use serde::{Deserialize, Serialize};

/// Configuration for the request pool (Redis or in-memory backend).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisPoolConfig {
    /// The URL of the Redis database, e.g. "redis://localhost:6379"
    pub redis_url: String,
    /// The TTL of the Redis database
    pub redis_ttl: u64,

    /// Whether to use redis-backend, otherwise memory-backend
    pub enable_redis_pool: bool,
}
