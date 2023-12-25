pub mod cached_rpc_provider;
pub mod mem_provider;

use anyhow::{anyhow, Result};

use crate::host::provider::Provider;

pub fn new_provider(
    cache_path: Option<String>,
    rpc_url: Option<String>,
) -> Result<Box<dyn Provider>> {
    match (cache_path, rpc_url) {
        (Some(cache_path), Some(rpc_url)) => {
            crate::host::provider::new_cached_rpc_provider(cache_path, rpc_url)
        }
        (Some(cache_path), None) => crate::host::provider::new_file_provider(cache_path),
        (None, Some(rpc_url)) => {
            let cache = mem_provider::MemProvider::new();
            cached_rpc_provider::CachedRpcProvider::new(Box::new(cache), rpc_url)
                .map(|p| Box::new(p) as Box<dyn Provider>)
        }
        (None, None) => Err(anyhow!("No cache_path or rpc_url given")),
    }
}
