use crate::{Pool, RedisPoolConfig};
use lazy_static::lazy_static;
use redis::{RedisError, RedisResult};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use lru::LruCache;

type SingleStorage = Arc<Mutex<LruCache<Value, Value>>>;
type GlobalStorage = Mutex<HashMap<String, SingleStorage>>;

lazy_static! {
    // #{redis_url => single_storage}
    //
    // We use redis_url to distinguish different redis database for tests, to prevent
    // data race problem when running multiple tests.
    static ref GLOBAL_STORAGE: GlobalStorage = Mutex::new(HashMap::new());
}

pub struct MemoryBackend {
    storage: SingleStorage,
}

impl MemoryBackend {
    pub fn new(redis_url: String) -> Self {
        let mut global = GLOBAL_STORAGE.lock().unwrap();

        let mem_capacity = std::env::var("MEMORY_BACKEND_SIZE")
            .unwrap_or("2048".to_string())
            .parse::<usize>()
            .unwrap_or_else(|_| 2048);
        Self {
            storage: global
                .entry(redis_url)
                .or_insert_with(|| {
                    Arc::new(Mutex::new(LruCache::new(
                        NonZeroUsize::new(mem_capacity).unwrap(),
                    )))
                })
                .clone(),
        }
    }

    pub fn set_ex<K: Serialize, V: Serialize>(
        &mut self,
        key: K,
        val: V,
        _ttl: u64,
    ) -> RedisResult<()> {
        let mut lock = self.storage.lock().unwrap();
        lock.put(json!(key), json!(val));
        Ok(())
    }

    pub fn get<K: Serialize, V: serde::de::DeserializeOwned>(&mut self, key: &K) -> RedisResult<V> {
        let mut lock = self.storage.lock().unwrap();
        match lock.get(&json!(key)) {
            None => Err(RedisError::from((redis::ErrorKind::TypeError, "not found"))),
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                RedisError::from((
                    redis::ErrorKind::TypeError,
                    "deserialization error",
                    e.to_string(),
                ))
            }),
        }
    }

    pub fn del<K: Serialize>(&mut self, key: K) -> RedisResult<usize> {
        let mut lock = self.storage.lock().unwrap();
        if lock.pop(&json!(key)).is_none() {
            Ok(0)
        } else {
            Ok(1)
        }
    }

    pub fn keys<K: serde::de::DeserializeOwned>(&mut self, key: &str) -> RedisResult<Vec<K>> {
        assert_eq!(key, "*", "memory backend only supports '*'");

        let lock = self.storage.lock().unwrap();
        Ok(lock
            .iter()
            .map(|(k, _)| serde_json::from_value(k.clone()).unwrap())
            .collect())
    }
}

/// Return the memory pool with the given id.
///
/// This is used for testing. Please use the test case name as the id to prevent data race.
pub fn memory_pool<S: ToString>(id: S) -> Pool {
    let config = RedisPoolConfig {
        redis_ttl: 111,
        redis_url: format!("redis://{}:6379", id.to_string()),
        enable_redis_pool: false,
    };
    Pool::open(config).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use redis::RedisResult;

    #[test]
    fn test_memory_pool() {
        let mut pool = memory_pool("test_memory_pool");
        let mut conn = pool.conn().expect("memory conn");

        let key = "hello".to_string();
        let val = "world".to_string();
        conn.set_ex(key.clone(), val.clone(), 111)
            .expect("memory set_ex");

        let actual: RedisResult<String> = conn.get(&key);
        assert_eq!(actual, Ok(val));

        let _ = conn.del(&key);
        let actual: RedisResult<String> = conn.get(&key);
        assert!(actual.is_err());
    }

    #[test]
    fn test_multiple_memory_pool() {
        let mut pool1 = memory_pool("test_multiple_memory_pool_1");
        let mut pool2 = memory_pool("test_multiple_memory_pool_2");
        let mut conn1 = pool1.conn().expect("memory conn");
        let mut conn2 = pool2.conn().expect("memory conn");

        let key = "hello".to_string();
        let world = "world".to_string();

        {
            conn1
                .set_ex(key.clone(), world.clone(), 111)
                .expect("memory set_ex");
            let actual: RedisResult<String> = conn1.get(&key);
            assert_eq!(actual, Ok(world.clone()));
        }

        {
            let actual: RedisResult<String> = conn2.get(&key);
            assert!(actual.is_err());
        }

        {
            let meme = "meme".to_string();
            conn2
                .set_ex(key.clone(), meme.clone(), 111)
                .expect("memory set_ex");
            let actual: RedisResult<String> = conn2.get(&key);
            assert_eq!(actual, Ok(meme));
        }

        {
            let actual: RedisResult<String> = conn1.get(&key);
            assert_eq!(actual, Ok(world));
        }
    }

    #[test]
    fn test_memory_pool_lru() {
        let mut pool = memory_pool("test_memory_pool");
        let mut conn = pool.conn().expect("memory conn");

        for i in 0..2048 {
            let key = format!("key{}", i);
            let val = format!("val{}", i);
            conn.set_ex(key.clone(), val.clone(), 111)
                .expect("memory set_ex");
        }

        for i in 0..2048 {
            let key = format!("key{}", i);
            let actual: RedisResult<String> = conn.get(&key);
            assert_eq!(actual, Ok(format!("val{}", i)));
        }

        for i in 2048..2048 + 10 {
            let key = format!("key{}", i);
            let val = format!("val{}", i);
            conn.set_ex(key.clone(), val.clone(), 111)
                .expect("memory set_ex");
        }

        for i in 0..10 {
            let key = format!("key{}", i);
            let actual: RedisResult<String> = conn.get(&key);
            assert!(actual.is_err());
        }

        for i in 10..2048 + 10 {
            let key = format!("key{}", i);
            let actual: RedisResult<String> = conn.get(&key);
            assert_eq!(actual, Ok(format!("val{}", i)));
        }
    }
}
