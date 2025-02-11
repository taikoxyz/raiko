/// This macro implements the Display trait for a type by using serde_json's pretty printing.
/// If the type cannot be serialized to JSON, it falls back to using Debug formatting.
///
/// The type must implement serde's Serialize trait for JSON serialization to work.
/// If serialization fails, it will fall back to using the Debug implementation.
#[macro_export]
macro_rules! impl_display_using_json_pretty {
    ($type:ty) => {
        impl std::fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match serde_json::to_string(self) {
                    Ok(s) => write!(f, "{}", s),
                    Err(_) => write!(f, "{:?}", self),
                }
            }
        }
    };
}
