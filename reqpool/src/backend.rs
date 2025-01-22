use crate::MemoryBackend;
use redis::{Commands, FromRedisValue, RedisResult, ToRedisArgs};
use serde::Serialize;

/// A connection wrapper that integrates both Redis and MemoryConnection.
pub enum Backend {
    Redis(redis::Connection),
    Memory(MemoryBackend),
}

impl Backend {
    pub fn set_ex<K: Serialize + ToRedisArgs, V: Serialize + ToRedisArgs>(
        &mut self,
        key: K,
        val: V,
        ttl: u64,
    ) -> RedisResult<()> {
        match self {
            Backend::Redis(conn) => conn.set_ex(key, val, ttl),
            Backend::Memory(conn) => conn.set_ex(key, val, ttl),
        }
    }

    pub fn get<
        K: Serialize + ToRedisArgs,
        V: serde::de::DeserializeOwned + ToRedisArgs + FromRedisValue,
    >(
        &mut self,
        key: &K,
    ) -> RedisResult<V> {
        match self {
            Backend::Redis(conn) => conn.get(key),
            Backend::Memory(conn) => conn.get(key),
        }
    }

    pub fn del<K: Serialize + ToRedisArgs>(&mut self, key: K) -> RedisResult<usize> {
        match self {
            Backend::Redis(conn) => conn.del(key),
            Backend::Memory(conn) => conn.del(key),
        }
    }

    pub fn keys<K: serde::de::DeserializeOwned + ToRedisArgs + FromRedisValue>(
        &mut self,
        key: &str,
    ) -> RedisResult<Vec<K>> {
        match self {
            Backend::Redis(conn) => conn.keys(key),
            Backend::Memory(conn) => conn.keys(key),
        }
    }
}
