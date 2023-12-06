use std::path::{absolute, PathBuf};

#[derive(Debug, Default)]
pub struct Context {
    /// guest executable path
    pub guest_path: PathBuf,
    /// cache for public input
    pub cache_path: PathBuf,
    pub sgx_context: SgxContext,
}

#[derive(Debug, Default)]
pub struct SgxContext {
    pub instance_id: u32,
}

impl Context {
    pub fn new(guest_path: PathBuf, cache_path: PathBuf, sgx_instance_id: u32) -> Self {
        let guest_path = absolute(guest_path).unwrap();
        let cache_path = absolute(cache_path).unwrap();
        Self {
            guest_path,
            cache_path,
            sgx_context: SgxContext {
                instance_id: sgx_instance_id,
            },
        }
    }
}
