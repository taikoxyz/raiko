//! Manifest types for encoding block proposals and metadata.
//!
//! These types are used for encoding/decoding derivation source manifests
//! in the Shasta protocol payload format.

use std::io::{Read, Write};

use alloy_primitives::{Address, Bytes, U256};
use alloy_rlp::{Decodable, Encodable, RlpDecodable, RlpEncodable};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::{
    constants::{PROPOSAL_MAX_BLOCKS, SHASTA_PAYLOAD_VERSION},
    error::Result,
};

/// Manifest of a single block proposal, matching `LibManifest.ProtocolBlockManifest`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, RlpEncodable, RlpDecodable)]
#[serde(rename_all = "camelCase")]
pub struct BlockManifest {
    /// Unix timestamp for this block.
    pub timestamp: u64,
    /// Coinbase (fee recipient) address.
    pub coinbase: Address,
    /// Anchor block number from L1.
    pub anchor_block_number: u64,
    /// Block gas limit.
    pub gas_limit: u64,
    /// Encoded transactions for this block.
    pub transactions: Bytes,
}

/// Manifest for a derivation source, matching `LibManifest.DerivationSourceManifest`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
#[serde(rename_all = "camelCase")]
pub struct DerivationSourceManifest {
    /// Block manifests in this derivation source.
    pub blocks: Vec<BlockManifest>,
}

impl DerivationSourceManifest {
    /// Create a new derivation source manifest.
    pub fn new(blocks: Vec<BlockManifest>) -> Self {
        Self { blocks }
    }

    /// Encode and compress the derivation source manifest following the Shasta protocol payload
    /// format.
    ///
    /// The format is:
    /// - 32 bytes: version (last byte is SHASTA_PAYLOAD_VERSION)
    /// - 32 bytes: length of compressed data (big-endian)
    /// - N bytes: zlib-compressed RLP-encoded manifest
    pub fn encode_and_compress(&self) -> Result<Vec<u8>> {
        encode_manifest_payload(self)
    }

    /// Decompress and decode a derivation source manifest from the Shasta protocol payload bytes.
    pub fn decompress_and_decode(bytes: &[u8], offset: usize) -> Result<Self> {
        let Some(decoded) = decode_manifest_payload(bytes, offset)? else {
            return Ok(DerivationSourceManifest::default());
        };

        let mut decoded_slice = decoded.as_slice();
        let manifest = match <DerivationSourceManifest as Decodable>::decode(&mut decoded_slice) {
            Ok(manifest) => manifest,
            Err(err) => {
                info!(
                    ?err,
                    "failed to decode derivation manifest rlp; returning default manifest"
                );
                return Ok(DerivationSourceManifest::default());
            }
        };

        if manifest.blocks.len() > PROPOSAL_MAX_BLOCKS {
            return Ok(DerivationSourceManifest::default());
        }

        Ok(manifest)
    }
}

/// Encode a manifest into the Shasta protocol payload format.
fn encode_manifest_payload<T>(manifest: &T) -> Result<Vec<u8>>
where
    T: Encodable,
{
    let rlp_encoded = alloy_rlp::encode(manifest);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&rlp_encoded)?;
    let compressed = encoder.finish()?;

    let mut output = Vec::with_capacity(64 + compressed.len());

    // Version bytes (32 bytes, version in last byte)
    let mut version_bytes = [0u8; 32];
    version_bytes[31] = SHASTA_PAYLOAD_VERSION;
    output.extend_from_slice(&version_bytes);

    // Length bytes (32 bytes, big-endian)
    let len_bytes = U256::from(compressed.len()).to_be_bytes::<32>();
    output.extend_from_slice(&len_bytes);
    output.extend_from_slice(&compressed);

    Ok(output)
}

/// Decode a manifest from the Shasta protocol payload format.
fn decode_manifest_payload(bytes: &[u8], offset: usize) -> Result<Option<Vec<u8>>> {
    if bytes.len() < offset + 64 {
        return Ok(None);
    }

    // Check version
    let version_raw = U256::from_be_slice(&bytes[offset..offset + 32]);
    let Ok(version) = u32::try_from(version_raw) else {
        return Ok(None);
    };
    if version != SHASTA_PAYLOAD_VERSION as u32 {
        return Ok(None);
    }

    // Get size
    let size_raw = U256::from_be_slice(&bytes[offset + 32..offset + 64]);
    let Ok(size_u64) = u64::try_from(size_raw) else {
        return Ok(None);
    };
    let Ok(size) = usize::try_from(size_u64) else {
        return Ok(None);
    };

    if bytes.len() < offset + 64 + size {
        return Ok(None);
    }

    // Decompress
    let compressed = &bytes[offset + 64..offset + 64 + size];
    let mut decoder = ZlibDecoder::new(compressed);
    let mut decoded = Vec::new();
    if decoder.read_to_end(&mut decoded).is_err() {
        return Ok(None);
    }

    Ok(Some(decoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derivation_source_manifest_encode_decode() {
        let manifest = DerivationSourceManifest::default();
        let encoded = manifest.encode_and_compress().unwrap();

        assert!(encoded.len() >= 64);
        assert_eq!(encoded[31], SHASTA_PAYLOAD_VERSION);

        let decoded = DerivationSourceManifest::decompress_and_decode(&encoded, 0).unwrap();
        assert_eq!(decoded.blocks.len(), manifest.blocks.len());
    }

    #[test]
    fn test_decode_manifest_payload_version_mismatch() {
        let mut payload = vec![0u8; 64];
        payload[31] = SHASTA_PAYLOAD_VERSION + 1;

        let decoded = decode_manifest_payload(&payload, 0).unwrap();
        assert!(decoded.is_none());
    }
}
