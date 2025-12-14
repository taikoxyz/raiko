#[cfg(feature = "std")]
use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "std")]
use serde_json::Value;

use crate::input::GuestBatchInput;
use crate::utils::blobs::{zlib_compress_data, zlib_decompress_data};

/// The `prover_args` key used to carry an encoded Shasta `GuestBatchInput`.
///
/// This is a temporary compatibility layer until we introduce a dedicated field in the request
/// schema. Keeping this as a constant avoids divergent spellings across crates.
pub const PROVER_ARG_SHASTA_GUEST_INPUT: &str = "shasta_guest_input";

/// Wrap an *already-encoded* Shasta guest input string into a `prover_args` value.
///
/// This is the common path in the host: sub-tasks return the encoded string, and we forward it
/// to the proof task without re-encoding.
#[cfg(feature = "std")]
pub fn encode_guest_input_str_to_prover_arg_value(encoded: &str) -> Result<Value, String> {
    serde_json::to_value(encoded)
        .map_err(|e| format!("failed to serialize guest input string: {e}"))
}

/// Encode a Shasta `GuestBatchInput` for transport via `prover_args`.
///
/// Format: `base64(zlib(bincode(GuestBatchInput)))`.
#[cfg(feature = "std")]
pub fn encode_guest_input_to_compress_b64_str(input: &GuestBatchInput) -> Result<String, String> {
    let raw = bincode::serialize(input)
        .map_err(|e| format!("failed to bincode-serialize shasta guest input: {e}"))?;
    let compressed = zlib_compress_data(&raw)
        .map_err(|e| format!("failed to zlib-compress shasta guest input: {e}"))?;
    Ok(general_purpose::STANDARD.encode(compressed))
}

/// Decode a Shasta `GuestBatchInput` from a `prover_args` value created from an encoded string
/// (typically via [`encode_guest_input_str_to_prover_arg_value`]).
#[cfg(feature = "std")]
pub fn decode_guest_input_from_prover_arg_value(value: &Value) -> Result<GuestBatchInput, String> {
    let b64 = value
        .as_str()
        .ok_or_else(|| "shasta_guest_input prover_arg must be a string".to_string())?;
    let compressed = general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("failed to base64-decode shasta_guest_input: {e}"))?;
    let raw = zlib_decompress_data(&compressed)
        .map_err(|e| format!("failed to zlib-decompress shasta_guest_input: {e}"))?;
    bincode::deserialize(&raw)
        .map_err(|e| format!("failed to bincode-deserialize shasta_guest_input: {e}"))
}
