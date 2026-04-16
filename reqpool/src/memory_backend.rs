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

#[cfg(target_os = "linux")]
use tracing::info;

#[cfg(target_os = "linux")]
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Once,
};

use lru::LruCache;

#[cfg(target_os = "linux")]
static MALLOC_TRIM_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "linux")]
static MALLOC_TRIM_PERIODIC_THREAD: Once = Once::new();

/// After dropping large `serde_json::Value`s, glibc often keeps freed heap for reuse so RSS
/// stays high. On Linux, `malloc_trim(0)` can return some of that memory to the kernel.
///
/// - `MEMORY_BACKEND_MALLOC_TRIM_INTERVAL`: trim after every Nth `set_ex` or successful `del`
///   (default: 200). Set to `0` to disable.
/// - `MEMORY_BACKEND_MALLOC_TRIM_PERIOD_SECS`: positive seconds → spawn one background thread that
///   sleeps that long, then trims, in a loop (e.g. `86400` for once per day). First trim runs after
///   the first sleep. Read once at startup when the memory backend is first constructed.
///
/// Linux (glibc) only; no-op on other targets.
#[cfg(target_os = "linux")]
fn ensure_periodic_malloc_trim_thread() {
    MALLOC_TRIM_PERIODIC_THREAD.call_once(|| {
        let period_secs: u64 = match std::env::var("MEMORY_BACKEND_MALLOC_TRIM_PERIOD_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&n| n > 0)
        {
            Some(s) => s,
            None => return,
        };
        let _ = std::thread::Builder::new()
            .name("raiko_malloc_trim".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(period_secs));
                unsafe {
                    libc::malloc_trim(0);
                }
                info!(
                    period_secs,
                    "memory backend: malloc_trim(0) (periodic MEMORY_BACKEND_MALLOC_TRIM_PERIOD_SECS)"
                );
            });
    });
}

#[cfg(not(target_os = "linux"))]
fn ensure_periodic_malloc_trim_thread() {}

/// After dropping large `serde_json::Value`s, glibc often keeps freed heap for reuse so RSS
/// stays high. On Linux, `malloc_trim(0)` can return some of that memory to the kernel.
///
/// `MEMORY_BACKEND_MALLOC_TRIM_INTERVAL`: trim after every Nth `set_ex` or successful `del`
/// (default: 200). Set to `0` to disable. Linux (glibc) only; no-op on other targets.
#[cfg(target_os = "linux")]
const DEFAULT_MALLOC_TRIM_INTERVAL: usize = 200;

#[cfg(target_os = "linux")]
fn maybe_malloc_trim_after_heap_release() {
    let interval: usize = std::env::var("MEMORY_BACKEND_MALLOC_TRIM_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MALLOC_TRIM_INTERVAL);
    if interval == 0 {
        return;
    }
    let n = MALLOC_TRIM_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    if n % interval == 0 {
        unsafe {
            libc::malloc_trim(0);
        }
        info!(
            interval,
            op_count = n,
            "memory backend: malloc_trim(0) (MEMORY_BACKEND_MALLOC_TRIM_INTERVAL)"
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn maybe_malloc_trim_after_heap_release() {}

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
        let storage = {
            let mut global = GLOBAL_STORAGE.lock().unwrap();

            let mem_capacity = std::env::var("MEMORY_BACKEND_SIZE")
                .unwrap_or("2048".to_string())
                .parse::<usize>()
                .unwrap_or_else(|_| 2048);
            global
                .entry(redis_url)
                .or_insert_with(|| {
                    Arc::new(Mutex::new(LruCache::new(
                        NonZeroUsize::new(mem_capacity).unwrap(),
                    )))
                })
                .clone()
        };
        ensure_periodic_malloc_trim_thread();
        Self { storage }
    }

    pub fn set_ex<K: Serialize, V: Serialize>(
        &mut self,
        key: K,
        val: V,
        _ttl: u64,
    ) -> RedisResult<()> {
        {
            let mut lock = self.storage.lock().unwrap();
            lock.put(json!(key), json!(val));
        }
        maybe_malloc_trim_after_heap_release();
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
        let removed = {
            let mut lock = self.storage.lock().unwrap();
            if lock.pop(&json!(key)).is_none() {
                0usize
            } else {
                1usize
            }
        };
        if removed > 0 {
            maybe_malloc_trim_after_heap_release();
        }
        Ok(removed)
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
