use alloy_rlp::{Decodable, Encodable};
use anyhow::Result;

use super::types::{ProtocolBlockManifest, ProtocolProposalManifest};
use crate::utils::{zlib_compress_data, zlib_decompress_data};

/// Encode and compress a Shasta proposal manifest (equivalent to Go's EncodeAndCompressShastaProposal)
pub fn encode_and_compress_shasta_proposal(proposal: &ProtocolProposalManifest) -> Result<Vec<u8>> {
    // First, RLP encode the proposal
    let rlp_encoded = alloy_rlp::encode(proposal);

    // Then compress using zlib
    let compressed = zlib_compress_data(&rlp_encoded)?;

    Ok(compressed)
}

/// Decode and decompress a Shasta proposal manifest
pub fn decode_and_decompress_shasta_proposal(
    compressed_data: &[u8],
) -> Result<ProtocolProposalManifest> {
    // First, decompress the data
    let rlp_encoded = zlib_decompress_data(compressed_data)?;

    // Then RLP decode
    let mut data = rlp_encoded.as_slice();
    let proposal = ProtocolProposalManifest::decode(&mut data)?;

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
        let header = alloy_rlp::Header { list: true, payload_length };
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
            return Err(alloy_rlp::Error::Custom("ProtocolBlockManifest must be encoded as a list"));
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

impl Encodable for ProtocolProposalManifest {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        // Calculate the payload length first
        let payload_length = self.prover_auth_bytes.length() + self.blocks.length();
        
        // Encode the list header
        let header = alloy_rlp::Header { list: true, payload_length };
        header.encode(out);
        
        // Encode fields in the same order as Go struct
        self.prover_auth_bytes.encode(out);
        self.blocks.encode(out);
    }
    
    fn length(&self) -> usize {
        let payload_length = self.prover_auth_bytes.length() + self.blocks.length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }
}

impl Decodable for ProtocolProposalManifest {
    fn decode(buf: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        // Decode the RLP header first
        let header = alloy_rlp::Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::Custom("ProtocolProposalManifest must be encoded as a list"));
        }
        
        // Decode each field sequentially
        Ok(ProtocolProposalManifest {
            prover_auth_bytes: alloy_primitives::Bytes::decode(buf)?,
            blocks: Vec::<ProtocolBlockManifest>::decode(buf)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, Bytes};

    fn create_test_proposal() -> ProtocolProposalManifest {
        let block = ProtocolBlockManifest {
            timestamp: 1234567890,
            coinbase: Address::from([1u8; 20]),
            anchor_block_number: 100,
            gas_limit: 30_000_000,
            transactions: vec![],
        };

        ProtocolProposalManifest {
            prover_auth_bytes: Bytes::from(vec![1, 2, 3, 4]),
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
        assert_eq!(original.prover_auth_bytes, decoded.prover_auth_bytes);
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
        let decoded = ProtocolProposalManifest::decode(&mut data).unwrap();

        // Verify roundtrip
        assert_eq!(original.prover_auth_bytes, decoded.prover_auth_bytes);
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

    #[test]
    fn test_rlp_encode_decode_shasta_from_client_data() {
        let bytes = include_bytes!("../../testdata/shasta_proposal_compressed.bin");
        let decoded = decode_and_decompress_shasta_proposal(bytes).unwrap();

        // Verify the decoded data matches expected values from Go test data
        assert_eq!(
            decoded.prover_auth_bytes,
            alloy_primitives::Bytes::from("test-prover-auth")
        );
        assert_eq!(decoded.blocks.len(), 2);

        // First block assertions
        let first_block = &decoded.blocks[0];
        assert_eq!(first_block.timestamp, 1234567890);
        assert_eq!(
            first_block.coinbase,
            Address::from_slice(&hex::decode("1234567890123456789012345678901234567890").unwrap())
        );
        assert_eq!(first_block.anchor_block_number, 100);
        assert_eq!(first_block.gas_limit, 8000000);
        assert_eq!(first_block.transactions.len(), 2);

        // Second block assertions
        let second_block = &decoded.blocks[1];
        assert_eq!(second_block.timestamp, 1234567900);
        assert_eq!(
            second_block.coinbase,
            Address::from_slice(&hex::decode("9876543210987654321098765432109876543210").unwrap())
        );
        assert_eq!(second_block.anchor_block_number, 0); // Using 0 to test the case where it uses the previous anchor
        assert_eq!(second_block.gas_limit, 10000000);
        assert_eq!(second_block.transactions.len(), 0);
    }
}
