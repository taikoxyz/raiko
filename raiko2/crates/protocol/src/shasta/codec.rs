//! Shasta event codec - optimized decoder for inbox events.
//!
//! These routines mirror the Solidity implementations found in
//! `contracts/layer1/core/impl/CodecOptimized.sol`.

use alloy_primitives::{Address, B256};

use super::{
    constants::MAX_BOND_TYPE,
    error::{ProtocolError, Result},
    types::{
        BlobSlice, BondInstruction, Checkpoint, CoreState, Derivation, DerivationSource, Proposal,
        ProposedEventPayload, ProvedEventPayload, Transition, TransitionMetadata, TransitionRecord,
    },
};

/// Decode a compactly encoded proposed event payload emitted by the inbox.
pub fn decode_proposed_event(data: &[u8]) -> Result<ProposedEventPayload> {
    let mut decoder = Decoder::new(data);

    // Proposal
    let proposal = Proposal {
        id: decoder.read_u48()?,
        timestamp: decoder.read_u48()?,
        end_of_submission_window_timestamp: decoder.read_u48()?,
        proposer: decoder.read_address()?,
        core_state_hash: decoder.read_bytes32()?,
        derivation_hash: decoder.read_bytes32()?,
    };

    // Derivation
    let origin_block_number = decoder.read_u48()?;
    let origin_block_hash = decoder.read_bytes32()?;
    let basefee_sharing_pctg = decoder.read_u8()?;

    let sources_len = decoder.read_u16()? as usize;
    let mut sources = Vec::with_capacity(sources_len);
    for _ in 0..sources_len {
        sources.push(DerivationSource {
            blob_slice: read_blob_slice(&mut decoder)?,
            flags: decoder.read_u8()?,
        });
    }

    let derivation = Derivation {
        origin_block_number,
        origin_block_hash,
        basefee_sharing_pctg,
        sources,
    };

    // CoreState
    let core_state = CoreState {
        next_proposal_id: decoder.read_u48()?,
        last_proposal_block_id: decoder.read_u48()?,
        last_finalized_proposal_id: decoder.read_u48()?,
        last_checkpoint_timestamp: decoder.read_u48()?,
        last_finalized_transition_hash: decoder.read_bytes32()?,
        bond_instructions_hash: decoder.read_bytes32()?,
    };

    // BondInstructions
    let bond_instructions = read_bond_instructions(&mut decoder, false)?;

    decoder.finish()?;

    Ok(ProposedEventPayload {
        proposal,
        derivation,
        core_state,
        bond_instructions,
    })
}

/// Decode a compactly encoded proved event payload emitted by the inbox.
pub fn decode_proved_event(data: &[u8]) -> Result<ProvedEventPayload> {
    let mut decoder = Decoder::new(data);

    let proposal_id = decoder.read_u48()?;

    let transition = Transition {
        proposal_hash: decoder.read_bytes32()?,
        parent_transition_hash: decoder.read_bytes32()?,
        checkpoint: Checkpoint {
            block_number: decoder.read_u48()?,
            block_hash: decoder.read_bytes32()?,
            state_root: decoder.read_bytes32()?,
        },
    };

    let mut transition_record = TransitionRecord {
        span: decoder.read_u8()?,
        bond_instructions: Vec::new(),
        transition_hash: decoder.read_bytes32()?,
        checkpoint_hash: decoder.read_bytes32()?,
    };

    let metadata = TransitionMetadata {
        designated_prover: decoder.read_address()?,
        actual_prover: decoder.read_address()?,
    };

    transition_record.bond_instructions = read_bond_instructions(&mut decoder, true)?;

    decoder.finish()?;

    Ok(ProvedEventPayload {
        proposal_id,
        transition,
        transition_record,
        metadata,
    })
}

/// Byte-slice cursor for decoding packed data.
#[derive(Clone, Copy, Debug)]
struct Decoder<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    fn ensure(&self, n: usize) -> Result<()> {
        if self.remaining() < n {
            return Err(ProtocolError::InsufficientBytes {
                expected: n,
                offset: self.offset,
                actual: self.remaining(),
            });
        }
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8> {
        self.ensure(1)?;
        let val = self.data[self.offset];
        self.offset += 1;
        Ok(val)
    }

    fn read_u16(&mut self) -> Result<u16> {
        self.ensure(2)?;
        let val = u16::from_be_bytes(self.data[self.offset..self.offset + 2].try_into().unwrap());
        self.offset += 2;
        Ok(val)
    }

    fn read_u24(&mut self) -> Result<u32> {
        self.ensure(3)?;
        let mut buf = [0u8; 4];
        buf[1..4].copy_from_slice(&self.data[self.offset..self.offset + 3]);
        self.offset += 3;
        Ok(u32::from_be_bytes(buf))
    }

    fn read_u48(&mut self) -> Result<u64> {
        self.ensure(6)?;
        let mut buf = [0u8; 8];
        buf[2..8].copy_from_slice(&self.data[self.offset..self.offset + 6]);
        self.offset += 6;
        Ok(u64::from_be_bytes(buf))
    }

    fn read_bytes32(&mut self) -> Result<B256> {
        self.ensure(32)?;
        let val = B256::from_slice(&self.data[self.offset..self.offset + 32]);
        self.offset += 32;
        Ok(val)
    }

    fn read_address(&mut self) -> Result<Address> {
        self.ensure(20)?;
        let val = Address::from_slice(&self.data[self.offset..self.offset + 20]);
        self.offset += 20;
        Ok(val)
    }

    fn finish(&self) -> Result<()> {
        if self.remaining() != 0 {
            return Err(ProtocolError::Other(format!(
                "trailing bytes: {} remaining",
                self.remaining()
            )));
        }
        Ok(())
    }
}

/// Read a BlobSlice structure.
fn read_blob_slice(decoder: &mut Decoder<'_>) -> Result<BlobSlice> {
    let blob_hashes_len = decoder.read_u16()? as usize;
    let mut blob_hashes = Vec::with_capacity(blob_hashes_len);
    for _ in 0..blob_hashes_len {
        blob_hashes.push(decoder.read_bytes32()?);
    }
    Ok(BlobSlice {
        blob_hashes,
        offset: decoder.read_u24()?,
        timestamp: decoder.read_u48()?,
    })
}

/// Decode a sequence of bond instructions.
fn read_bond_instructions(
    decoder: &mut Decoder<'_>,
    enforce_type: bool,
) -> Result<Vec<BondInstruction>> {
    let len = decoder.read_u16()? as usize;
    let mut instructions = Vec::with_capacity(len);
    for _ in 0..len {
        instructions.push(read_bond_instruction(decoder, enforce_type)?);
    }
    Ok(instructions)
}

/// Decode a single bond instruction.
fn read_bond_instruction(decoder: &mut Decoder<'_>, enforce_type: bool) -> Result<BondInstruction> {
    let proposal_id = decoder.read_u48()?;
    let bond_type = decoder.read_u8()?;

    if enforce_type && bond_type > MAX_BOND_TYPE {
        return Err(ProtocolError::InvalidBondType(bond_type));
    }

    let payer = decoder.read_address()?;
    let payee = decoder.read_address()?;

    Ok(BondInstruction {
        proposal_id,
        bond_type,
        payer,
        payee,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_read_u48() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let mut decoder = Decoder::new(&data);
        let val = decoder.read_u48().unwrap();
        assert_eq!(val, 0x010203040506);
    }

    #[test]
    fn test_decoder_insufficient_bytes() {
        let data = [0x01, 0x02];
        let mut decoder = Decoder::new(&data);
        assert!(decoder.read_u48().is_err());
    }
}
