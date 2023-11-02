#[derive(Debug, Default)]
pub struct Context {
    pub guest_path: String,
    pub cache_path: String,
    pub sgx_context: SgxContext,
}

#[derive(Debug, Default)]
pub struct SgxContext {
    pub instance_id: u32,
}

impl Context {
    pub fn new(guest_path: String, cache_path: String, sgx_instance_id: u32) -> Self {
        Self {
            guest_path,
            cache_path,
            sgx_context: SgxContext {
                instance_id: sgx_instance_id,
            },
        }
    }
}
