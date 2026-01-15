use alloy_rlp::{Decodable, Encodable};
use anyhow::Result;

use super::types::{DerivationSourceManifest, ProtocolBlockManifest};
use crate::utils::blobs::{zlib_compress_data, zlib_decompress_data};

/// Encode and compress a Shasta proposal manifest (equivalent to Go's EncodeAndCompressShastaProposal)
pub fn encode_and_compress_shasta_proposal(proposal: &DerivationSourceManifest) -> Result<Vec<u8>> {
    // First, RLP encode the proposal
    let rlp_encoded = alloy_rlp::encode(proposal);

    // Then compress using zlib
    let compressed = zlib_compress_data(&rlp_encoded)?;

    Ok(compressed)
}

/// Decode and decompress a Shasta proposal manifest
pub fn decode_and_decompress_shasta_proposal(
    compressed_data: &[u8],
) -> Result<DerivationSourceManifest> {
    // First, decompress the data
    let rlp_encoded = zlib_decompress_data(compressed_data)?;

    // Then RLP decode
    let mut data = rlp_encoded.as_slice();
    let proposal = DerivationSourceManifest::decode(&mut data)?;

    Ok(proposal)
}

impl Encodable for ProtocolBlockManifest {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        // Calculate the payload length first
        let payload_length = self.timestamp.length()
            + self.coinbase.length()
            + self.anchor_block_number.length()
            + self.gas_limit.length()
            + self.transactions.length();

        // Encode the list header
        let header = alloy_rlp::Header {
            list: true,
            payload_length,
        };
        header.encode(out);

        // Encode fields in the same order as Go struct
        self.timestamp.encode(out);
        self.coinbase.encode(out);
        self.anchor_block_number.encode(out);
        self.gas_limit.encode(out);
        self.transactions.encode(out);
    }

    fn length(&self) -> usize {
        let payload_length = self.timestamp.length()
            + self.coinbase.length()
            + self.anchor_block_number.length()
            + self.gas_limit.length()
            + self.transactions.length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }
}

impl Decodable for ProtocolBlockManifest {
    fn decode(buf: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        // Decode the RLP header first
        let header = alloy_rlp::Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::Custom(
                "ProtocolBlockManifest must be encoded as a list",
            ));
        }

        // Decode each field sequentially
        Ok(ProtocolBlockManifest {
            timestamp: u64::decode(buf)?,
            coinbase: alloy_primitives::Address::decode(buf)?,
            anchor_block_number: u64::decode(buf)?,
            gas_limit: u64::decode(buf)?,
            transactions: Vec::<reth_primitives::TransactionSigned>::decode(buf)?,
        })
    }
}

impl Encodable for DerivationSourceManifest {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        // Calculate the payload length first
        let payload_length = self.blocks.length();

        // Encode the list header
        let header = alloy_rlp::Header {
            list: true,
            payload_length,
        };
        header.encode(out);

        // Encode fields in the same order as Go struct

        self.blocks.encode(out);
    }

    fn length(&self) -> usize {
        let payload_length = self.blocks.length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }
}

pub(crate) const PROPOSAL_MAX_BLOCKS: usize = 384;

impl Decodable for DerivationSourceManifest {
    fn decode(buf: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        // Decode the RLP header first
        let header = alloy_rlp::Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::Custom(
                "ProtocolProposalManifest must be encoded as a list",
            ));
        }

        let blocks = Vec::<ProtocolBlockManifest>::decode(buf)?;
        if blocks.len() > PROPOSAL_MAX_BLOCKS {
            return Err(alloy_rlp::Error::Custom(
                "ProtocolProposalManifest blocks length exceeds PROPOSAL_MAX_BLOCKS",
            ));
        }

        // Decode each field sequentially
        Ok(DerivationSourceManifest { blocks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;

    fn create_test_proposal() -> DerivationSourceManifest {
        let block = ProtocolBlockManifest {
            timestamp: 1234567890,
            coinbase: Address::from([1u8; 20]),
            anchor_block_number: 100,
            gas_limit: 30_000_000,
            transactions: vec![],
        };

        DerivationSourceManifest {
            blocks: vec![block],
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = create_test_proposal();

        // Encode and compress
        let encoded = encode_and_compress_shasta_proposal(&original).unwrap();

        // Decode and decompress
        let decoded = decode_and_decompress_shasta_proposal(&encoded).unwrap();

        // Verify roundtrip
        assert_eq!(original.blocks.len(), decoded.blocks.len());

        if !original.blocks.is_empty() && !decoded.blocks.is_empty() {
            let orig_block = &original.blocks[0];
            let decoded_block = &decoded.blocks[0];

            assert_eq!(orig_block.timestamp, decoded_block.timestamp);
            assert_eq!(orig_block.coinbase, decoded_block.coinbase);
            assert_eq!(
                orig_block.anchor_block_number,
                decoded_block.anchor_block_number
            );
            assert_eq!(orig_block.gas_limit, decoded_block.gas_limit);
        }
    }

    #[test]
    fn test_rlp_encode_decode() {
        let original = create_test_proposal();

        // RLP encode
        let rlp_encoded = alloy_rlp::encode(&original);

        // RLP decode
        let mut data = rlp_encoded.as_slice();
        let decoded = DerivationSourceManifest::decode(&mut data).unwrap();

        // Verify roundtrip
        assert_eq!(original.blocks.len(), decoded.blocks.len());
    }

    #[test]
    fn test_encode_decode_roundtrip_shasta() {
        let original = create_test_proposal();

        // Encode and compress
        let encoded = encode_and_compress_shasta_proposal(&original).unwrap();

        // Decode and decompress
        let _decoded = decode_and_decompress_shasta_proposal(&encoded).unwrap();
    }
}
