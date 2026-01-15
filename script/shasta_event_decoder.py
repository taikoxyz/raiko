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
class InboxConfig:
    """IInbox Config structure - matches IInbox.sol Config struct"""
    codec: str  # address
    bond_token: str  # address
    checkpoint_store: str  # address (signal service)
    proof_verifier: str  # address
    proposer_checker: str  # address
    proving_window: int  # uint48
    extended_proving_window: int  # uint48
    max_finalization_count: int  # uint256
    finalization_grace_period: int  # uint48
    ring_buffer_size: int  # uint256
    basefee_sharing_pctg: int  # uint8
    min_forced_inclusion_count: int  # uint256
    forced_inclusion_delay: int  # uint16
    forced_inclusion_fee_in_gwei: int  # uint64
    min_checkpoint_delay: int  # uint16
    permissionless_inclusion_multiplier: int  # uint8
    composite_key_version: int  # uint16

@dataclass
class ShastaProposal:
    """Shasta Proposal structure - matches IInbox.sol Proposal struct (simplified version without coreStateHash)"""
    id: int  # uint48
    timestamp: int  # uint48
    end_of_submission_window_timestamp: int  # uint48
    proposer: str  # address as hex string
    parent_proposal_hash: str  # bytes32 as hex string
    derivation_hash: str  # bytes32 as hex string
    # Note: coreStateHash has been removed from the on-chain version

@dataclass
class BlobSlice:
    """LibBlobs.BlobSlice - subset of fields we care about"""
    blob_hashes: List[str]
    offset: int
    timestamp: int

@dataclass
class DerivationSource:
    """Derivation Source structure - matches IInbox.sol DerivationSource struct"""
    is_forced_inclusion: bool
    blob_slice: BlobSlice  # Contains blob hashes plus offset/timestamp metadata

@dataclass
class ShastaDerivation:
    """Shasta Derivation structure - matches IInbox.sol Derivation struct"""
    origin_block_number: int  # uint48
    origin_block_hash: str  # bytes32 as hex string
    basefee_sharing_pctg: int  # uint8
    sources: List[DerivationSource]  # DerivationSource[]

@dataclass
class ShastaCoreState:
    """Shasta Core State structure - matches IInbox.sol CoreState struct"""
    next_proposal_id: int  # uint48
    last_proposal_block_id: int  # uint48
    last_finalized_proposal_id: int  # uint48
    last_checkpoint_timestamp: int  # uint48
    last_finalized_transition_hash: str  # bytes32 as hex string
    bond_instructions_hash: str  # bytes32 as hex string

@dataclass
class ShastaEventData:
    """Complete Shasta Event Data structure - matches IInbox.ProposedEventPayload (without CoreState)"""
    proposal: ShastaProposal
    derivation: ShastaDerivation
    # Note: CoreState is not included in the on-chain encoding

@dataclass
class Checkpoint:
    """Checkpoint structure - matches ICheckpointStore.Checkpoint"""
    block_number: int  # uint48
    block_hash: str  # bytes32 as hex string
    state_root: str  # bytes32 as hex string

@dataclass
class Transition:
    """Transition structure - matches IInbox.sol Transition struct"""
    proposal_hash: str  # bytes32 as hex string
    parent_transition_hash: str  # bytes32 as hex string
    checkpoint: Checkpoint

@dataclass
class TransitionMetadata:
    """Transition Metadata structure - matches IInbox.sol TransitionMetadata struct"""
    designated_prover: str  # address as hex string
    actual_prover: str  # address as hex string

@dataclass
class BondInstruction:
    """Bond Instruction structure - matches LibBonds.BondInstruction"""
    # This would need to be defined based on LibBonds.BondInstruction
    # For now, using a generic dict
    instruction: dict

@dataclass
class TransitionRecord:
    """Transition Record structure - matches IInbox.sol TransitionRecord struct"""
    span: int  # uint8
    bond_instructions: List[BondInstruction]  # LibBonds.BondInstruction[]
    transition_hash: str  # bytes32 as hex string
    checkpoint_hash: str  # bytes32 as hex string

@dataclass
class TransitionRecordHash:
    """Transition Record Hash structure"""
    finalization_deadline: int  # uint48
    record_hash: str  # bytes26 as hex string

@dataclass
class ProposedEventPayload:
    """Proposed Event Payload structure - matches IInbox.sol ProposedEventPayload"""
    proposal: ShastaProposal
    derivation: ShastaDerivation

@dataclass
class ProvedEventPayload:
    """Proved Event Payload structure - matches IInbox.sol ProvedEventPayload"""
    proposal_id: int  # uint48
    transition: Transition
    transition_record: TransitionRecord
    metadata: TransitionMetadata

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
    
    def unpack_uint8(self, data: bytes, pos: int) -> Tuple[int, int]:
        """Unpack an 8-bit unsigned integer (1 byte)"""
        if pos + 1 > len(data):
            raise ValueError("Not enough data to read 1-byte uint8")
        
        value = data[pos]
        new_pos = pos + 1
        return value, new_pos
    
    def unpack_uint16(self, data: bytes, pos: int) -> Tuple[int, int]:
        """Unpack a 16-bit unsigned integer (2 bytes)"""
        if pos + 2 > len(data):
            raise ValueError("Not enough data to read 2-byte uint16")
        
        value = struct.unpack('>H', data[pos:pos+2])[0]
        new_pos = pos + 2
        return value, new_pos
    
    def unpack_uint64(self, data: bytes, pos: int) -> Tuple[int, int]:
        """Unpack a 64-bit unsigned integer (8 bytes)"""
        if pos + 8 > len(data):
            raise ValueError("Not enough data to read 8-byte uint64")
        
        value = struct.unpack('>Q', data[pos:pos+8])[0]
        new_pos = pos + 8
        return value, new_pos
    
    def unpack_uint256(self, data: bytes, pos: int) -> Tuple[int, int]:
        """Unpack a 256-bit unsigned integer (32 bytes)"""
        if pos + 32 > len(data):
            raise ValueError("Not enough data to read 32-byte uint256")
        
        # Convert 32 bytes to int (big-endian)
        value = int.from_bytes(data[pos:pos+32], byteorder='big')
        new_pos = pos + 32
        return value, new_pos
    
    def unpack_bytes26(self, data: bytes, pos: int) -> Tuple[str, int]:
        """Unpack a 26-byte value"""
        if pos + 26 > len(data):
            raise ValueError("Not enough data to read 26-byte value")
        
        bytes_26 = data[pos:pos+26]
        hex_str = '0x' + bytes_26.hex()
        new_pos = pos + 26
        return hex_str, new_pos
    
    def decode_event_data(self, data: bytes) -> ShastaEventData:
        """
        Decode Shasta event data following the custom encoding format.
        Based on the decode function in LibProposedEventEncoder.sol
        
        Encoding order matches IInbox.ProposedEventPayload:
        1. Proposal: (id, proposer, timestamp, endOfSubmissionWindowTimestamp, parentProposalHash)
        2. Derivation: (originBlockNumber, originBlockHash, basefeeSharingPctg, sources[])
        3. Proposal.derivationHash (after derivation data)
        """
        try:
            ptr = 0
            
            # Decode Proposal fields
            proposal_id, ptr = self.unpack_uint48(data, ptr)
            proposer, ptr = self.unpack_address(data, ptr)
            timestamp, ptr = self.unpack_uint48(data, ptr)
            end_of_submission_window_timestamp, ptr = self.unpack_uint48(data, ptr)
            parent_proposal_hash, ptr = self.unpack_hash(data, ptr)
            
            # Decode Derivation fields
            origin_block_number, ptr = self.unpack_uint48(data, ptr)
            origin_block_hash, ptr = self.unpack_hash(data, ptr)
            basefee_sharing_pctg, ptr = self.unpack_uint8(data, ptr)
            
            # Decode sources array length
            sources_length, ptr = self.unpack_uint16(data, ptr)
            
            sources = []
            for _ in range(sources_length):
                # Decode is_forced_inclusion flag
                is_forced_inclusion_u8, ptr = self.unpack_uint8(data, ptr)
                is_forced_inclusion = is_forced_inclusion_u8 != 0
                
                # Decode blob slice for this source
                blob_hashes_length, ptr = self.unpack_uint16(data, ptr)
                
                blob_hashes = []
                for _ in range(blob_hashes_length):
                    blob_hash, ptr = self.unpack_hash(data, ptr)
                    blob_hashes.append(blob_hash)
                
                offset, ptr = self.unpack_uint24(data, ptr)
                blob_timestamp, ptr = self.unpack_uint48(data, ptr)
                
                sources.append(
                    DerivationSource(
                        is_forced_inclusion=is_forced_inclusion,
                        blob_slice=BlobSlice(
                            blob_hashes=blob_hashes,
                            offset=offset,
                            timestamp=blob_timestamp,
                        ),
                    )
                )
            
            # Decode Proposal.derivationHash (after derivation sources)
            derivation_hash, ptr = self.unpack_hash(data, ptr)
            
            # Note: CoreState has been removed from the on-chain ProposedEventPayload encoding
            # The decode function only decodes: Proposal + Derivation (without CoreState)
            
            # Create the structures
            proposal = ShastaProposal(
                id=proposal_id,
                timestamp=timestamp,
                end_of_submission_window_timestamp=end_of_submission_window_timestamp,
                proposer=proposer,
                parent_proposal_hash=parent_proposal_hash,
                derivation_hash=derivation_hash
            )
            
            derivation = ShastaDerivation(
                origin_block_number=origin_block_number,
                origin_block_hash=origin_block_hash,
                basefee_sharing_pctg=basefee_sharing_pctg,
                sources=sources
            )
            
            return ShastaEventData(
                proposal=proposal,
                derivation=derivation
            )
            
        except Exception as e:
            raise ValueError(f"Failed to decode Shasta event data: {e}")
    
    def decode_config_from_abi_response(self, abi_response: List) -> InboxConfig:
        """Decode config from ABI response (getConfig function)"""
        if len(abi_response) != 17:
            raise ValueError(f"Expected 17 config parameters, got {len(abi_response)}")
        
        return InboxConfig(
            codec=abi_response[0],
            bond_token=abi_response[1],
            checkpoint_store=abi_response[2],
            proof_verifier=abi_response[3],
            proposer_checker=abi_response[4],
            proving_window=abi_response[5],
            extended_proving_window=abi_response[6],
            max_finalization_count=abi_response[7],
            finalization_grace_period=abi_response[8],
            ring_buffer_size=abi_response[9],
            basefee_sharing_pctg=abi_response[10],
            min_forced_inclusion_count=abi_response[11],
            forced_inclusion_delay=abi_response[12],
            forced_inclusion_fee_in_gwei=abi_response[13],
            min_checkpoint_delay=abi_response[14],
            permissionless_inclusion_multiplier=abi_response[15],
            composite_key_version=abi_response[16]
        )
    
    def decode_transition_record_hash_from_abi_response(self, abi_response: Tuple) -> TransitionRecordHash:
        """Decode transition record hash from ABI response (getTransitionRecordHash function)"""
        if len(abi_response) != 2:
            raise ValueError(f"Expected 2 transition record hash parameters, got {len(abi_response)}")
        
        return TransitionRecordHash(
            finalization_deadline=abi_response[0],
            record_hash=abi_response[1]
        )
    
    def extract_batch_id(self, data: bytes) -> Optional[int]:
        """
        Extract the batch ID (proposal ID) from the event data
        This is a convenience method for the stress script
        """
        try:
            # Fast-path: first field is uint48 proposal id (6 bytes big-endian)
            if len(data) < 6:
                return None
            return int.from_bytes(data[0:6], byteorder="big")
        except Exception as e:
            print(f"Error extracting batch ID: {e}")
            return None

def test_decoder():
    """Test the decoder with the event data we found earlier"""
    
    # This is the event data we extracted from block 8281
    # You can replace this with actual event data
    test_data_hex = "0x0000000009143c44cdddb6a900fa2b585dd299e03d12fa4293bc0000693e56cc000000000000d1374b45317e657e07505c83fc4702e8f6e043ff3e7beb2eaa0974783a4222ae0000000038aeb5f96a8745b06f7a00e5741f503d6d45c0d5ec1377960abe86e45299d6410cdf4b000100000101b1a43b3e87672be8a5102ac0d99dc4215491d8a07a7fa402d34d7f1ac9696d0000000000693e56cc36cf931b08528aa49160c33ecda1505b2c292a4947c416d9dc26646ebe9c0d35"
    
    print("🧪 Testing Shasta Event Decoder")
    print("=" * 40)
    
    try:
        # Convert hex string to bytes
        test_data = bytes.fromhex(test_data_hex.replace("0x", ""))
        
        decoder = ShastaEventDecoder()
        event_data = decoder.decode_event_data(test_data)
        
        print("✅ Successfully decoded event data!")
        print(f"Proposal ID (Batch ID): {event_data.proposal.id}")
        print(f"Proposer: {event_data.proposal.proposer}")
        print(f"Timestamp: {event_data.proposal.timestamp}")
        print(f"End of Submission Window: {event_data.proposal.end_of_submission_window_timestamp}")
        print(f"Parent Proposal Hash: {event_data.proposal.parent_proposal_hash}")
        print(f"Derivation Hash: {event_data.proposal.derivation_hash}")
        print(f"Origin Block Number: {event_data.derivation.origin_block_number}")
        print(f"Origin Block Hash: {event_data.derivation.origin_block_hash}")
        print(f"Basefee Sharing Pctg: {event_data.derivation.basefee_sharing_pctg}")
        print(f"Derivation Sources Count: {len(event_data.derivation.sources)}")
        
    except Exception as e:
        print(f"❌ Test failed: {e}")

if __name__ == "__main__":
    test_decoder()
