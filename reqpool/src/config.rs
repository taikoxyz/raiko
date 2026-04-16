use serde::{Deserialize, Serialize};

/// Configuration for the request pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisPoolConfig {
    /// The URL of the Redis database, e.g. "redis://localhost:6379"
    pub redis_url: String,
    /// The TTL for mirrored Redis keys
    pub redis_ttl: u64,

    /// When false: in-memory LRU only.
    /// When true: in-memory LRU for all keys, and additionally mirror `ShastaProof` /
    /// `ShastaAggregation` to Redis (guest input and other keys stay memory-only; reads try memory
    /// then Redis for mirrored key types).
    pub enable_redis_pool: bool,
}
