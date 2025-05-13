#![cfg(feature = "statedb_lru")]
use lazy_static::lazy_static;
use std::{collections::HashMap, num::NonZeroUsize, sync::Mutex};

use raiko_lib::mem_db::MemDb;
use reth_primitives::{Header, B256};
use tracing::debug;

use lru::LruCache;

type ChainBlockCacheKey = (u64, B256);
type ChainBlockCacheEntry = (MemDb, HashMap<u64, Header>);

lazy_static! {
    static ref HISTORY_STATE_DB: Mutex<LruCache<ChainBlockCacheKey, ChainBlockCacheEntry>> =
        Mutex::new(LruCache::<ChainBlockCacheKey, ChainBlockCacheEntry>::new(
            NonZeroUsize::new(256).unwrap()
        ));
}

fn clear_state_db() {
    debug!("clear state db");
    let mut hashmap = HISTORY_STATE_DB.lock().unwrap();
    hashmap.clear();
}

pub(crate) fn save_state_db(key: ChainBlockCacheKey, value: ChainBlockCacheEntry) {
    debug!("save state db key: {:?}", key);
    let mut hashmap = HISTORY_STATE_DB.lock().unwrap();
    tracing::trace!("save state db account: {:?}", value.0.accounts);
    tracing::trace!("save state history headers: {:?}", value.1);
    hashmap.put(key, value);
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
    use serial_test::serial;

    #[test]
    #[serial]
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
    #[serial]
    fn test_lru_cache_replace() {
        clear_state_db();
        for i in 0..256 + 4 {
            let key = (i, B256::ZERO);
            let value = (MemDb::default(), HashMap::new());
            save_state_db(key, value.clone());
        }
        // 0 is out
        let key = (0, B256::ZERO);
        assert!(load_state_db(key).is_none());

        // 1 is out
        let key = (1, B256::ZERO);
        assert!(load_state_db(key).is_none());

        // 4 is still in
        let key = (4, B256::ZERO);
        assert!(load_state_db(key).is_some());
    }
}
