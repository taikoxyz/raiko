use std::path::{absolute, PathBuf};

use tracing::debug;

#[derive(Debug, Clone)]
pub struct Context {
    /// guest executable path
    pub guest_path: PathBuf,
    /// cache for public input
    pub cache_path: PathBuf,

    pub guest_context: GuestContext,
}

#[derive(Debug, Clone)]
pub enum GuestContext {
    Sgx(SgxContext),
    Powdr(PowdrContext)
}

#[derive(Debug, Default, Clone)]
pub struct SgxContext {
    pub instance_id: u32,
}
#[derive(Debug, Default, Clone)]
pub struct PowdrContext {
    // TODO
}

impl Context {
    pub fn new_sgx(guest_path: PathBuf, cache_path: PathBuf, sgx_instance_id: u32) -> Self {
        let guest_path = absolute(guest_path).unwrap();
        debug!("Guest path: {:?}", guest_path);
        let cache_path = absolute(cache_path).unwrap();
        debug!("Cache path: {:?}", cache_path);
        Self {
            guest_path,
            cache_path,
            guest_context: GuestContext::Sgx(SgxContext {
                instance_id: sgx_instance_id,
            })
        }
    }

    pub fn new_powdr(guest_path: PathBuf, cache_path: PathBuf) -> Self {
        let guest_path = absolute(guest_path).unwrap();
        debug!("Guest path: {:?}", guest_path);
        let cache_path = absolute(cache_path).unwrap();
        debug!("Cache path: {:?}", cache_path);
        Self {
            guest_path,
            cache_path,
            guest_context: GuestContext::Powdr(PowdrContext {
                // TODO
            })
        }
    }
}
