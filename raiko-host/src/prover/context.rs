use std::path::{absolute, PathBuf};

use tracing::debug;

#[derive(Debug, Default, Clone)]
pub struct Context {
    /// guest executable path
    pub guest_path: PathBuf,
    /// cache for public input
    pub cache_path: PathBuf,
    pub sgx_context: SgxContext,
    pub log_path: Option<PathBuf>,
}

#[derive(Debug, Default, Clone)]
pub struct SgxContext {
    pub instance_id: u32,
}

impl Context {
    pub fn new(
        guest_path: PathBuf,
        cache_path: PathBuf,
        log_path: Option<PathBuf>,
        sgx_instance_id: u32,
    ) -> Self {
        let guest_path = absolute(guest_path).unwrap();
        debug!("Guest path: {:?}", guest_path);
        let cache_path = absolute(cache_path).unwrap();
        debug!("Cache path: {:?}", cache_path);
        Self {
            guest_path,
            cache_path,
            log_path,
            sgx_context: SgxContext {
                instance_id: sgx_instance_id,
            },
        }
    }
}
