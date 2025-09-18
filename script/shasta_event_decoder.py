#!/usr/bin/env python3

"""
Shasta Event Decoder for Python
Based on the Rust implementation in shasta.rs

This decodes the bytes data from the Proposed event to extract
the Shasta proposal information including the batch ID.
"""

import struct
from typing import Tuple, List, Optional
from dataclasses import dataclass

@dataclass
class ShastaProposal:
    """Shasta Proposal structure"""
    id: int
    timestamp: int
    end_of_submission_window_timestamp: int
    proposer: str  # Address as hex string
    core_state_hash: str  # Hash as hex string
    derivation_hash: str  # Hash as hex string

@dataclass
class ShastaDerivation:
    """Shasta Derivation structure"""
    origin_block_number: int
    origin_block_hash: str  # Hash as hex string
    is_forced_inclusion: bool
    basefee_sharing_pctg: int
    blob_hashes: List[str]  # List of hashes as hex strings
    offset: int
    blob_timestamp: int

@dataclass
class ShastaCoreState:
    """Shasta Core State structure"""
    next_proposal_id: int
    last_finalized_proposal_id: int
    last_finalized_transition_hash: str  # Hash as hex string
    bond_instructions_hash: str  # Hash as hex string

@dataclass
class ShastaEventData:
    """Complete Shasta Event Data structure"""
    proposal: ShastaProposal
    derivation: ShastaDerivation
    core_state: ShastaCoreState

class ShastaEventDecoder:
    """Decoder for Shasta event data"""
    
    def __init__(self):
        pass
    
    def unpack_uint24(self, data: bytes, pos: int) -> Tuple[int, int]:
        """Unpack a 24-bit unsigned integer (3 bytes)"""
        if pos + 3 > len(data):
            raise ValueError("Not enough data to read 3-byte uint24")
        
        # Read 3 bytes and convert to uint24 (big-endian)
        value = struct.unpack('>I', b'\x00' + data[pos:pos+3])[0]
        new_pos = pos + 3
        return value, new_pos
    
    def unpack_uint48(self, data: bytes, pos: int) -> Tuple[int, int]:
        """Unpack a 48-bit unsigned integer (6 bytes)"""
        if pos + 6 > len(data):
            raise ValueError("Not enough data to read 6-byte uint48")
        
        # Read 6 bytes and convert to uint48 (big-endian)
        value = struct.unpack('>Q', b'\x00\x00' + data[pos:pos+6])[0]
        new_pos = pos + 6
        return value, new_pos
    
    def unpack_address(self, data: bytes, pos: int) -> Tuple[str, int]:
        """Unpack a 20-byte address"""
        if pos + 20 > len(data):
            raise ValueError("Not enough data to read 20-byte address")
        
        address_bytes = data[pos:pos+20]
        address = '0x' + address_bytes.hex()
        new_pos = pos + 20
        return address, new_pos
    
    def unpack_hash(self, data: bytes, pos: int) -> Tuple[str, int]:
        """Unpack a 32-byte hash"""
        if pos + 32 > len(data):
            raise ValueError("Not enough data to read 32-byte hash")
        
        hash_bytes = data[pos:pos+32]
        hash_hex = '0x' + hash_bytes.hex()
        new_pos = pos + 32
        return hash_hex, new_pos
    
    def decode_event_data(self, data: bytes) -> ShastaEventData:
        """
        Decode Shasta event data following the Rust implementation
        """
        try:
            ptr = 0
            
            # Decode Proposal
            proposal_id, ptr = self.unpack_uint48(data, ptr)
            proposer, ptr = self.unpack_address(data, ptr)
            timestamp, ptr = self.unpack_uint48(data, ptr)
            end_of_submission_window_timestamp, ptr = self.unpack_uint48(data, ptr)
            
            # Decode Derivation
            origin_block_number, ptr = self.unpack_uint48(data, ptr)
            origin_block_hash, ptr = self.unpack_hash(data, ptr)
            
            is_forced_inclusion = data[ptr] != 0
            ptr += 1
            basefee_sharing_pctg = data[ptr]
            ptr += 1
            
            blob_hashes_length, ptr = self.unpack_uint24(data, ptr)
            
            blob_hashes = []
            for _ in range(blob_hashes_length):
                blob_hash, ptr = self.unpack_hash(data, ptr)
                blob_hashes.append(blob_hash)
            
            offset, ptr = self.unpack_uint24(data, ptr)
            blob_timestamp, ptr = self.unpack_uint48(data, ptr)
            core_state_hash, ptr = self.unpack_hash(data, ptr)
            derivation_hash, ptr = self.unpack_hash(data, ptr)
            
            # Decode Core State
            next_proposal_id, ptr = self.unpack_uint48(data, ptr)
            last_finalized_proposal_id, ptr = self.unpack_uint48(data, ptr)
            last_finalized_transition_hash, ptr = self.unpack_hash(data, ptr)
            bond_instructions_hash, ptr = self.unpack_hash(data, ptr)
            
            # Create the structures
            proposal = ShastaProposal(
                id=proposal_id,
                timestamp=timestamp,
                end_of_submission_window_timestamp=end_of_submission_window_timestamp,
                proposer=proposer,
                core_state_hash=core_state_hash,
                derivation_hash=derivation_hash
            )
            
            derivation = ShastaDerivation(
                origin_block_number=origin_block_number,
                origin_block_hash=origin_block_hash,
                is_forced_inclusion=is_forced_inclusion,
                basefee_sharing_pctg=basefee_sharing_pctg,
                blob_hashes=blob_hashes,
                offset=offset,
                blob_timestamp=blob_timestamp
            )
            
            core_state = ShastaCoreState(
                next_proposal_id=next_proposal_id,
                last_finalized_proposal_id=last_finalized_proposal_id,
                last_finalized_transition_hash=last_finalized_transition_hash,
                bond_instructions_hash=bond_instructions_hash
            )
            
            return ShastaEventData(
                proposal=proposal,
                derivation=derivation,
                core_state=core_state
            )
            
        except Exception as e:
            raise ValueError(f"Failed to decode Shasta event data: {e}")
    
    def extract_batch_id(self, data: bytes) -> Optional[int]:
        """
        Extract the batch ID (proposal ID) from the event data
        This is a convenience method for the stress script
        """
        try:
            event_data = self.decode_event_data(data)
            return event_data.proposal.id
        except Exception as e:
            print(f"Error extracting batch ID: {e}")
            return None

def test_decoder():
    """Test the decoder with the event data we found earlier"""
    
    # This is the event data we extracted from block 8281
    # You can replace this with actual event data
    test_data_hex = "0000000009c33c44cdddb6a900fa2b585dd299e03d12fa4293bc000068cac6f800000000000000000000205811ea9b03bfa6..."
    
    print("🧪 Testing Shasta Event Decoder")
    print("=" * 40)
    
    try:
        # Convert hex string to bytes
        test_data = bytes.fromhex(test_data_hex.replace('0x', ''))
        
        decoder = ShastaEventDecoder()
        event_data = decoder.decode_event_data(test_data)
        
        print("✅ Successfully decoded event data!")
        print(f"Proposal ID (Batch ID): {event_data.proposal.id}")
        print(f"Proposer: {event_data.proposal.proposer}")
        print(f"Timestamp: {event_data.proposal.timestamp}")
        print(f"Origin Block Number: {event_data.derivation.origin_block_number}")
        print(f"Blob Hashes Count: {len(event_data.derivation.blob_hashes)}")
        
    except Exception as e:
        print(f"❌ Test failed: {e}")

if __name__ == "__main__":
    test_decoder()
