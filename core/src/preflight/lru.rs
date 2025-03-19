#![cfg(feature = "statedb_lru")]
use lazy_static::lazy_static;
use std::{collections::HashMap, sync::Mutex};

use raiko_lib::mem_db::MemDb;
use reth_primitives::{Header, B256};
use tracing::debug;

use lru_time_cache::LruCache;

type ChainBlockCacheKey = (u64, B256);
type ChainBlockCacheEntry = (MemDb, HashMap<u64, Header>);

lazy_static! {
    static ref HISTORY_STATE_DB: Mutex<LruCache<ChainBlockCacheKey, ChainBlockCacheEntry>> =
        Mutex::new(LruCache::<ChainBlockCacheKey, ChainBlockCacheEntry>::with_capacity(16));
}

pub(crate) fn save_state_db(key: ChainBlockCacheKey, value: ChainBlockCacheEntry) {
    debug!("save state db: ({:?} => {:?})", key, value);
    let mut hashmap = HISTORY_STATE_DB.lock().unwrap();
    tracing::info!("save state db: {:?}", value.0.accounts);
    hashmap.insert(key, value);
}

pub(crate) fn load_state_db(key: ChainBlockCacheKey) -> Option<ChainBlockCacheEntry> {
    debug!("query state db key: {:?}", key);
    let mut hashmap = HISTORY_STATE_DB.lock().unwrap();
    hashmap.get(&key).cloned()
}

#[cfg(test)]
mod test {
    use super::*;
    use raiko_lib::mem_db::MemDb;
    use reth_primitives::B256;

    #[test]
    fn test_lru_cache_save_load() {
        let key = (1, B256::ZERO);
        let value = (MemDb::default(), HashMap::new());
        save_state_db(key, value.clone());
        let result = load_state_db(key);
        assert!(result.is_some());
        assert_eq!(result.clone().unwrap().0.block_hashes, value.0.block_hashes);
        assert_eq!(
            result
                .clone()
                .unwrap()
                .0
                .accounts
                .keys()
                .collect::<Vec<_>>(),
            value.0.accounts.keys().collect::<Vec<_>>()
        );

        let key = (1, B256::random());
        assert!(load_state_db(key).is_none());
    }

    #[test]
    fn test_lru_cache_replace() {
        for i in 0..20 {
            let key = (i, B256::ZERO);
            let value = (MemDb::default(), HashMap::new());
            save_state_db(key, value.clone());
        }
        // 0 is out
        let key = (0, B256::ZERO);
        assert!(load_state_db(key).is_none());

        // 4 is still in
        let key = (4, B256::ZERO);
        assert!(load_state_db(key).is_some());
    }
}
