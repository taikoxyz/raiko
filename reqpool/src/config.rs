use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// The configuration for the redis-backend request pool
pub struct RedisPoolConfig {
    /// The URL of the Redis database, e.g. "redis://localhost:6379"
    pub redis_url: String,
    /// The TTL of the Redis database
    pub redis_ttl: u64,
    /// Whether to use memory-backend instead of redis-backend
    ///
    /// When true, the pool will use the memory-backend instead of the redis-backend.
    /// When false, the pool will use the redis-backend.
    pub enable_memory_backend: bool,
}
