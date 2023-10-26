#[derive(Debug, Default)]
pub struct Context {
    pub guest_path: String,
    pub cache_path: String,
}

impl Context {
    pub fn new(guest_path: String, cache_path: String) -> Self {
        Self {
            guest_path,
            cache_path,
        }
    }
}
