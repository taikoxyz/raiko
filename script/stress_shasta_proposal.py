import requests
import time
from datetime import datetime
import json
import logging
from typing import Optional, Dict, Any, Tuple
from collections import deque
import asyncio
import argparse
from dataclasses import dataclass
from random import random
import web3
from web3 import Web3
from web3.middleware import ExtraDataToPOAMiddleware
import sys
import os
from shasta_event_decoder import ShastaEventDecoder


@dataclass
class RaikoResponse:
    status: str
    data: Optional[Dict[str, Any]] = None
    message: Optional[str] = None


@dataclass
class AnchorTxInfo:
    """Information extracted from anchor transaction"""
    proposal_id: int
    anchor_number: int
    l2_block_number: int


@dataclass
class ProposalGroup:
    """Group of L2 blocks with the same proposal_id"""
    proposal_id: int
    anchor_number: int
    l2_block_numbers: list[int]


class BatchMonitor:
    def __init__(
        self,
        l1_rpc: str,
        l2_rpc: str,
        abi_file: str,
        evt_address: str,
        raiko_rpc: str,
        log_file: str = "block_monitor.log",
        polling_interval: int = 3,
        max_retries: int = 3,
        block_running_ratio: float = 0.1,
        l2_block_range: Optional[Tuple[int, int]] = None,
        timeout: int = 3600,  # 1 hour
        prove_type: str = "native",
        watch_mode: bool = False,
        time_speed: float = 1.0,
        anchor_abi_file: Optional[str] = None,
        aggregate: int = 0,
    ):
        self.l1_rpc = l1_rpc
        self.l2_rpc = l2_rpc
        self.raiko_rpc = raiko_rpc
        self.log_file = log_file
        self.block_polling_interval = polling_interval
        self.task_polling_interval = polling_interval
        self.max_retries = max_retries
        self.timeout = timeout
        self.last_processed_l2_block = None
        self.batchs_in_last_block = deque()
        self.block_running_ratio = block_running_ratio
        self.l2_block_range = l2_block_range
        self.ts_offset: Optional[int] = None
        self.last_block_ts_in_real_world: int = 0
        self.running_count = 0
        self.prove_type = prove_type
        self.watch_mode = watch_mode
        self.time_speed = time_speed
        self.anchor_abi_file = anchor_abi_file
        self.aggregate = aggregate
        # Initialize Shasta event decoder
        self.shasta_decoder = ShastaEventDecoder()
        # Cache for proposal block numbers: proposal_id -> l1_block_number
        # Used for both normal proposals and bond proposals
        self.proposal_block_cache: Dict[int, Optional[int]] = {}
        # Queue for proposals waiting to be aggregated
        self.pending_proposals: list[Dict[str, Any]] = []
        # Aggregate running count
        self.aggregate_running_count = 0
        # Track aggregate requests: list of proposal data dicts
        self.aggregate_requests: list[list[Dict[str, Any]]] = []
        # Track aggregate requests: list of proposal_ids lists
        self.aggregate_requests: list[list[int]] = []
        # logger
        logging.basicConfig(
            level=logging.INFO,
            format="%(asctime)s - %(levelname)s - %(message)s",
            handlers=[logging.FileHandler(log_file), logging.StreamHandler()],
        )
        self.logger = logging.getLogger(__name__)
        self.__init_contract_event(l1_rpc, abi_file, evt_address)

    def _extract_proposal_id_from_proposed_log(self, log) -> Optional[int]:
        """
        Extract proposal id from a Proposed event log.

        New ABI (plain event): `Proposed(uint48 id, address proposer, uint48 endOfSubmissionWindowTimestamp, uint8 basefeeSharingPctg, DerivationSource[] sources)`
        Old ABI (bytes payload): `Proposed(bytes data)` where proposal id is inside `data`.
        """
        try:
            # New/plain event path
            if hasattr(log, "args") and hasattr(log.args, "id") and log.args.id is not None:
                return int(log.args.id)

            # Backward-compatible path: old `data: bytes` payload
            if hasattr(log, "args") and hasattr(log.args, "data") and log.args.data is not None:
                event_data = log.args.data
                decoded = self.shasta_decoder.extract_batch_id(event_data)
                return int(decoded) if decoded is not None else None
        except Exception as e:
            self.logger.debug(f"Failed to extract proposal id from log: {e}")
            return None
        return None

    def __init_contract_event(self, l1_rpc, abi_file, evt_address):
        print(f"l1_rpc = {l1_rpc}, abi_file = {abi_file}, evt_address = {evt_address}")
        with open(abi_file) as f:
            abi = json.load(f)
        l1_w3 = Web3(Web3.HTTPProvider(l1_rpc, {"timeout": 10}))
        l1_w3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)
        if l1_w3.is_connected():
            self.logger.info(f"Connected to l1 node {l1_rpc}")
        self.evt_contract = l1_w3.eth.contract(address=evt_address, abi=abi["abi"])
        # Initialize L2 Web3 connection
        self.l2_w3 = Web3(Web3.HTTPProvider(self.l2_rpc, {"timeout": 10}))
        if self.l2_w3.is_connected():
            self.logger.info(f"Connected to l2 node {self.l2_rpc}")
        
        # Load anchor ABI if provided, otherwise try to use the event ABI
        if self.anchor_abi_file:
            self.logger.info(f"Loading anchor ABI from {self.anchor_abi_file}")
            try:
                with open(self.anchor_abi_file) as f:
                    anchor_abi_data = json.load(f)
                    self.l2_abi = anchor_abi_data.get("abi", anchor_abi_data if isinstance(anchor_abi_data, list) else [])
            except Exception as e:
                self.logger.warning(f"Could not load anchor ABI file {self.anchor_abi_file}: {e}")
                self.l2_abi = abi.get("abi", [])
        else:
            # Fallback to event ABI (might not have anchorV4 function)
            self.logger.info("No anchor ABI file provided, using event ABI (may not have anchorV4)")
            self.l2_abi = abi.get("abi", [])
        
        # Try to create L2 contract instance for decoding (address doesn't matter for decoding)
        try:
            self.l2_contract = self.l2_w3.eth.contract(
                address=Web3.to_checksum_address("0x0000000000000000000000000000000000000000"), 
                abi=self.l2_abi
            )
            # Check if anchorV4 function exists
            if hasattr(self.l2_contract.functions, 'anchorV4'):
                self.logger.info("anchorV4 function found in ABI, will use web3.py decoding")
            else:
                self.logger.warning("anchorV4 function not found in ABI, will use manual decoding")
        except Exception as e:
            self.logger.warning(f"Could not initialize L2 contract for decoding: {e}")
            self.l2_contract = None

    async def get_l2_block(self, block_number: int) -> Optional[Dict[str, Any]]:
        """Get L2 block by number"""
        try:
            response = requests.post(
                self.l2_rpc,
                json={
                    "jsonrpc": "2.0",
                    "method": "eth_getBlockByNumber",
                    "params": [hex(block_number), True],  # True to include full transaction data
                    "id": 1,
                },
                timeout=10,
            )
            result = response.json()
            return result.get("result")
        except Exception as e:
            self.logger.error(f"Failed to get L2 block {block_number}: {e}")
            return None

    def extract_proposal_id_from_extradata(self, extradata: str) -> Optional[int]:
        """
        Extract proposal_id from block extradata.
        
        Format: byte[0] is config, bytes[1:7] is proposal_id (6 bytes, big-endian uint48)
        Example: 0x4b000000000005 -> proposal_id = 5
        
        Returns proposal_id or None if extraction fails.
        """
        try:
            # Remove 0x prefix if present
            if extradata.startswith("0x"):
                extradata = extradata[2:]
            
            extradata_bytes = bytes.fromhex(extradata)
            
            # Need at least 7 bytes (1 config byte + 6 proposal_id bytes)
            if len(extradata_bytes) < 7:
                self.logger.warning(f"extradata too short: {len(extradata_bytes)} bytes (need at least 7)")
                return None
            
            # Extract proposal_id from bytes[1:7] (6 bytes, big-endian)
            proposal_id_bytes = extradata_bytes[1:7]
            proposal_id = int.from_bytes(proposal_id_bytes, byteorder="big")
            
            self.logger.debug(f"Extracted proposal_id from extradata: {proposal_id}")
            return proposal_id
        except Exception as e:
            self.logger.error(f"Error extracting proposal_id from extradata: {e}")
            import traceback
            self.logger.debug(traceback.format_exc())
            return None

    def decode_anchor_tx_input(self, tx_input: str) -> Optional[int]:
        """
        Decode anchorV4 transaction input to extract anchor_number.
        
        New ABI only has _checkpoint parameter, which contains blockNumber.
        
        First tries to use web3.py's contract decoding if ABI is available.
        Falls back to manual decoding if that fails.
        
        Returns anchor_number or None if decoding fails.
        """
        try:
            # Try web3.py contract decoding first if available
            if self.l2_contract is not None:
                try:
                    self.logger.info(f"Attempting Web3.py decoding for tx input (length: {len(tx_input)} chars)")
                    
                    # Decode the function call
                    func_obj, func_params = self.l2_contract.decode_function_input(tx_input)
                    self.logger.info(f"Successfully decoded function: {func_obj.fn_name}")
                    
                    # Check if it's anchorV4
                    if func_obj.fn_name == "anchorV4":
                        # New anchorV4 ABI: only has `_checkpoint` parameter (ICheckpointStore.Checkpoint)
                        # Checkpoint: (blockNumber: uint48, blockHash: bytes32, stateRoot: bytes32)
                        checkpoint_params = func_params.get("_checkpoint", {})
                        
                        self.logger.debug(
                            f"Decoded params - checkpoint_params type: {type(checkpoint_params)}"
                        )
                        
                        anchor_number = None
                        
                        # Extract blockNumber from checkpoint
                        if isinstance(checkpoint_params, (list, tuple)):
                            anchor_number = checkpoint_params[0] if len(checkpoint_params) > 0 else None
                            self.logger.debug(f"Extracted anchor_number from checkpoint tuple index 0: {anchor_number}")
                        elif isinstance(checkpoint_params, dict):
                            anchor_number = checkpoint_params.get("blockNumber")
                            self.logger.debug(f"Extracted anchor_number from checkpoint dict: {anchor_number}")
                        elif hasattr(checkpoint_params, 'blockNumber'):
                            anchor_number = checkpoint_params.blockNumber
                            self.logger.debug(f"Extracted anchor_number from checkpoint attribute: {anchor_number}")
                        
                        if anchor_number is not None:
                            self.logger.info(
                                f"✓ Decoded via ABI: anchor_number={anchor_number}"
                            )
                            return anchor_number
                        else:
                            self.logger.warning(
                                f"anchorV4 decoded but missing anchor_number"
                            )
                    else:
                        self.logger.warning(f"Function is {func_obj.fn_name}, not anchorV4")
                except Exception as e:
                    import traceback
                    self.logger.warning(f"Web3.py decoding failed: {e}")
                    self.logger.debug(f"Web3.py decoding traceback:\n{traceback.format_exc()}")
            
            # Fallback to manual decoding
            # Remove 0x prefix if present
            if tx_input.startswith("0x"):
                tx_input = tx_input[2:]
            
            input_bytes = bytes.fromhex(tx_input)
            
            # New anchorV4 ABI (updated):
            #   anchorV4(ICheckpointStore.Checkpoint)
            #   Checkpoint: (blockNumber: uint48, blockHash: bytes32, stateRoot: bytes32)
            #
            # ABI layout after selector (3 * 32 bytes = 96 bytes):
            #   word0: checkpoint.blockNumber (uint48 in low 6 bytes)   <-- anchor_number
            #   word1: checkpoint.blockHash (bytes32)
            #   word2: checkpoint.stateRoot (bytes32)
            # Total: 4 (selector) + 96 = 100 bytes
            
            if len(input_bytes) < 4 + 32 * 3:
                self.logger.error(f"Anchor tx input too short: {len(input_bytes)} bytes (expected at least {4 + 32 * 3})")
                return None
            
            # Get function selector
            func_selector = input_bytes[0:4].hex()
            self.logger.debug(f"Function selector: 0x{func_selector}")
            
            params_start = 4
            # word0: checkpoint.blockNumber (anchor number)
            checkpoint_block_number_word = input_bytes[params_start:params_start + 32]
            anchor_number = int.from_bytes(checkpoint_block_number_word[26:32], byteorder="big")
            
            self.logger.debug(
                f"Decoded anchor tx (manual): anchor_number={anchor_number}"
            )
            
            return anchor_number
        except Exception as e:
            self.logger.error(f"Error decoding anchor tx input: {e}")
            import traceback
            self.logger.debug(traceback.format_exc())
            return None

    async def parse_l2_block_anchor_tx(self, l2_block_number: int) -> Optional[AnchorTxInfo]:
        """
        Parse L2 block to extract anchor transaction information.
        Returns AnchorTxInfo with proposal_id, anchor_number, and l2_block_number.
        
        proposal_id is extracted from block extradata (bytes[1:7]).
        anchor_number is extracted from anchor transaction checkpoint.
        """
        try:
            block = await self.get_l2_block(l2_block_number)
            if block is None:
                return None
            
            # Extract proposal_id from block extradata
            extradata = block.get("extraData", "0x")
            if not extradata or extradata == "0x":
                self.logger.warning(f"No extradata in L2 block {l2_block_number}")
                return None
            
            proposal_id = self.extract_proposal_id_from_extradata(extradata)
            if proposal_id is None:
                self.logger.warning(f"Failed to extract proposal_id from extradata in block {l2_block_number}")
                return None
            
            transactions = block.get("transactions", [])
            if len(transactions) == 0:
                self.logger.warning(f"No transactions in L2 block {l2_block_number}")
                return None
            
            # First transaction is the anchor transaction
            anchor_tx = transactions[0]
            tx_input = anchor_tx.get("input", "0x")
            
            if tx_input == "0x" or len(tx_input) <= 2:
                self.logger.warning(f"Empty or invalid anchor tx input in block {l2_block_number}")
                return None
            
            # Decode anchor tx input to get anchor_number
            anchor_number = self.decode_anchor_tx_input(tx_input)
            if anchor_number is None:
                self.logger.warning(f"Failed to decode anchor tx input for block {l2_block_number}")
                return None
            
            return AnchorTxInfo(
                proposal_id=proposal_id,
                anchor_number=anchor_number,
                l2_block_number=l2_block_number
            )
        except Exception as e:
            self.logger.error(f"Error parsing L2 block {l2_block_number}: {e}")
            import traceback
            self.logger.debug(traceback.format_exc())
            return None

    def group_blocks_by_proposal_id(
        self, anchor_infos: list[AnchorTxInfo]
    ) -> list[ProposalGroup]:
        """
        Group consecutive L2 blocks by proposal_id.
        The start point is the first block with a new proposal_id,
        and the end point is the last block with the same proposal_id.
        """
        if not anchor_infos:
            return []
        
        groups = []
        current_proposal_id = None
        current_group = None
        
        for info in anchor_infos:
            if current_proposal_id is None or info.proposal_id != current_proposal_id:
                # Start a new group
                if current_group is not None:
                    groups.append(current_group)
                
                current_proposal_id = info.proposal_id
                current_group = ProposalGroup(
                    proposal_id=info.proposal_id,
                    anchor_number=info.anchor_number,
                    l2_block_numbers=[info.l2_block_number]
                )
            else:
                # Continue current group
                current_group.l2_block_numbers.append(info.l2_block_number)
                # Update anchor_number to use the first one in the group
                # (all blocks in a group should have the same anchor_number)
        
        # Add the last group
        if current_group is not None:
            groups.append(current_group)
        
        return groups

    async def find_l1_inclusion_block(
        self, proposal_id: int, anchor_number: int
    ) -> Optional[int]:
        """
        Find L1 inclusion block for a proposal_id.
        First checks cache, then searches if not cached.
        """
        # Check cache first
        if proposal_id in self.proposal_block_cache:
            cached_result = self.proposal_block_cache[proposal_id]
            if cached_result is not None:
                self.logger.debug(
                    f"Using cached L1 inclusion block {cached_result} for proposal_id {proposal_id}"
                )
            return cached_result
        
        # Not in cache, do individual search (fallback, should rarely happen if batch query worked)
        search_start = anchor_number + 1
        search_end = anchor_number + 128
        
        self.logger.info(
            f"Searching L1 blocks {search_start} to {search_end} for proposal_id {proposal_id} (not in cache)"
        )
        
        try:
            # Get events in the range
            logs = self.evt_contract.events.Proposed.get_logs(
                from_block=search_start, to_block=search_end
            )
            
            for log in logs:
                try:
                    decoded_proposal_id = self._extract_proposal_id_from_proposed_log(log)
                    
                    if decoded_proposal_id == proposal_id:
                        l1_block_number = log.blockNumber
                        self.proposal_block_cache[proposal_id] = l1_block_number
                        self.logger.info(
                            f"Found proposal_id {proposal_id} in L1 block {l1_block_number}"
                        )
                        return l1_block_number
                except Exception as e:
                    self.logger.error(f"Error decoding event log: {e}")
                    continue
            
            self.logger.warning(
                f"Proposal_id {proposal_id} not found in L1 blocks {search_start} to {search_end}"
            )
            self.proposal_block_cache[proposal_id] = None
            return None
        except Exception as e:
            self.logger.error(f"Error searching L1 events: {e}")
            self.proposal_block_cache[proposal_id] = None
            return None
    
    async def batch_find_proposal_blocks(
        self, proposal_queries: list[Tuple[int, int]], search_start: int, search_end: int
    ) -> Dict[int, Optional[int]]:
        """
        Batch query multiple proposal IDs to find their L1 inclusion blocks.
        
        Args:
            proposal_queries: List of (proposal_id, anchor_number) tuples
            search_start: Starting L1 block number to search
            search_end: Ending L1 block number to search
        
        Returns:
            Dictionary mapping proposal_id -> l1_block_number (or None if not found)
        """
        proposal_ids_to_find = {proposal_id for proposal_id, _ in proposal_queries}
        # Filter out already cached proposals
        uncached_queries = [
            (proposal_id, anchor_number)
            for proposal_id, anchor_number in proposal_queries
            if proposal_id not in self.proposal_block_cache
        ]
        
        if not uncached_queries:
            self.logger.info("All proposals already in cache, skipping batch query")
            return {
                proposal_id: self.proposal_block_cache.get(proposal_id)
                for proposal_id in proposal_ids_to_find
            }
        
        uncached_proposal_ids = {proposal_id for proposal_id, _ in uncached_queries}
        self.logger.info(
            f"Batch querying {len(uncached_proposal_ids)} proposals in L1 blocks {search_start} to {search_end}: {list(uncached_proposal_ids)}"
        )
        
        try:
            logs = self.evt_contract.events.Proposed.get_logs(
                from_block=search_start, to_block=search_end
            )
            
            # Build a map of proposal_id -> block_number from all logs
            proposal_to_block = {}
            for log in logs:
                try:
                    decoded_proposal_id = self._extract_proposal_id_from_proposed_log(log)
                    if decoded_proposal_id in uncached_proposal_ids:
                        # Only keep the first occurrence (earliest block)
                        if decoded_proposal_id not in proposal_to_block:
                            proposal_to_block[decoded_proposal_id] = log.blockNumber
                            self.logger.debug(
                                f"Found proposal_id {decoded_proposal_id} in L1 block {log.blockNumber}"
                            )
                except Exception as e:
                    self.logger.debug(f"Error decoding event log in batch search: {e}")
                    continue
            
            # Cache all found proposals (including None for not found)
            for proposal_id in uncached_proposal_ids:
                if proposal_id in proposal_to_block:
                    self.proposal_block_cache[proposal_id] = proposal_to_block[proposal_id]
                    self.logger.info(
                        f"Cached proposal_id {proposal_id} -> L1 block {proposal_to_block[proposal_id]}"
                    )
                else:
                    self.proposal_block_cache[proposal_id] = None
                    self.logger.warning(
                        f"Proposal_id {proposal_id} not found in batch search, cached as None"
                    )
        except Exception as e:
            self.logger.warning(f"Error in batch proposal search: {e}")
            # Mark all uncached as None if batch search fails
            for proposal_id in uncached_proposal_ids:
                self.proposal_block_cache[proposal_id] = None
        
        # Return results for all requested proposal IDs (including cached ones)
        return {
            proposal_id: self.proposal_block_cache.get(proposal_id)
            for proposal_id in proposal_ids_to_find
        }

    def parse_batch_proposed_meta(self, log):
        try:
            parsed_log = self.evt_contract.events.Proposed.process_log(log)
            # New/plain event: `id` is the proposal id
            if hasattr(parsed_log.args, "id"):
                return int(parsed_log.args.id)
            # Old/bytes event: decode from `data`
            if hasattr(parsed_log.args, "data"):
                decoded = self.shasta_decoder.extract_batch_id(parsed_log.args.data)
                return int(decoded) if decoded is not None else None
        except Exception as e:
            return None

    def get_batch_events_in_block(self, block_number) -> list[int]:
        try:
            logs = self.evt_contract.events.Proposed.get_logs(
                from_block=block_number, to_block=block_number
            )

            batch_ids = []
            for log in logs:
                try:
                    batch_id = self._extract_proposal_id_from_proposed_log(log)
                    if batch_id is not None:
                        batch_ids.append(batch_id)
                        self.logger.info(f"Decoded batch ID: {batch_id}")
                    else:
                        self.logger.warning(
                            f"Failed to decode batch ID from event data"
                        )
                except Exception as e:
                    self.logger.error(f"Error decoding event data: {e}")
                    continue

            return batch_ids
        except Exception as e:
            self.logger.error(f"Error getting events from block {block_number}: {e}")
            return []

    async def get_next_batches(self) -> Optional[tuple[int, list[int]]]:
        """get latest block number"""
        if self.block_range is not None:
            return await self.get_in_range_next_batch()
        else:
            return await self.get_latest_block_batchs()

    async def get_block(self, block_number) -> Optional[Dict[str, Any]]:
        """get block by number"""
        try:
            response = requests.post(
                self.l1_rpc,
                json={
                    "jsonrpc": "2.0",
                    "method": "eth_getBlockByNumber",
                    "params": [hex(block_number), False],
                    "id": 1,
                },
                timeout=10,
            )
            result = response.json()
            return result.get("result")
        except Exception as e:
            self.logger.error(f"Failed to get block {block_number}: {e}")

    async def align_ts_offset(self, first_block: int) -> bool:
        # query block timestamp
        try:
            block = await self.get_block(first_block)
            timestamp = int(block["timestamp"], 16)
            current_timestamp = int(time.time())
            self.ts_offset = current_timestamp - timestamp
            self.last_block_ts_in_real_world = current_timestamp
            self.logger.info(
                f"Begin timestamp: {timestamp}, timestamp offset: {self.ts_offset}"
            )
            return True
        except Exception as e:
            self.logger.error(f"align_ts_offset from {first_block} failed: {e}")
            return False

    async def get_in_range_next_batch(self) -> Optional[tuple[int, list[int]]]:
        """get latest block number"""
        start_block, end_block = self.block_range

        if self.last_block is None:
            next_block = start_block
        else:
            next_block = self.last_block + 1
            # align block timestamp offset after first block gets processed
            if self.ts_offset is None:
                if not await self.align_ts_offset(start_block):
                    return None

        while True:
            if next_block >= end_block:
                self.logger.info(f"Block range {self.block_range} finished")
                if self.running_count == 0:
                    self.logger.info("All blocks finished, exiting")
                    exit(0)
                return None
            else:
                # check if events exist in this block
                batch_ids = self.get_batch_events_in_block(next_block)
                if len(batch_ids) == 0:
                    self.logger.info(f"No batch events in block {next_block}")
                    next_block += 1
                    continue

                if self.ts_offset is None:
                    if not await self.align_ts_offset(next_block):
                        return None

                # check if next block timestamp reached
                block = await self.get_block(next_block)
                current_block_ts = int(block["timestamp"], 16)
                current_block_ts_in_real_world = current_block_ts + self.ts_offset
                real_world_ts = int(time.time())
                real_world_elapsed_time = (
                    real_world_ts - self.last_block_ts_in_real_world
                )
                accel_elapsed_time = real_world_elapsed_time * self.time_speed
                self.logger.info(
                    f"real_world_elapsed_time = {real_world_elapsed_time}, accel_world_elapsed_time = {accel_elapsed_time}"
                )
                current_accel_ts = self.last_block_ts_in_real_world + accel_elapsed_time
                if current_accel_ts >= current_block_ts_in_real_world:
                    self.last_block = next_block
                    self.last_block_ts_in_real_world = real_world_ts
                    self.ts_offset = real_world_ts - current_block_ts
                    self.logger.info(
                        f"last block processing timestamp in stress = {self.last_block_ts_in_real_world}"
                    )
                    return self.last_block, batch_ids
                else:
                    self.logger.info(
                        f"Block {next_block} timestamp: {current_block_ts_in_real_world} is not reached, current: {current_accel_ts}, sleep {current_block_ts_in_real_world - current_accel_ts} sec."
                    )
                    self.last_block = next_block - 1
                    await asyncio.sleep(
                        (current_block_ts_in_real_world - current_accel_ts)
                        / self.time_speed
                    )
                    return None

    async def get_latest_block_batchs(self) -> Optional[tuple[int, list[int]]]:
        """get latest block number"""
        logs = self.evt_contract.events.Proposed().get_logs(
            from_block="latest", to_block="latest"
        )
        if len(logs) == 0:
            return None

        batch_ids = []
        for log in logs:
            try:
                batch_id = self._extract_proposal_id_from_proposed_log(log)
                if batch_id is not None:
                    batch_ids.append(batch_id)
                    self.logger.info(f"Decoded batch ID: {batch_id}")
                else:
                    self.logger.warning(f"Failed to decode batch ID from event data")
            except Exception as e:
                self.logger.error(f"Error decoding event data: {e}")
                continue

        return logs[0].blockNumber, batch_ids

    def generate_post_data(
        self, 
        proposals: list[Dict[str, Any]], 
        aggregate: bool = False
    ) -> Dict[str, Any]:
        """generate post data"""
        return {
            "proposals": proposals,
            "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
            "graffiti": "8008500000000000000000000000000000000000000000000000000000000000",
            "proof_type": self.prove_type,
            "blob_proof_type": "proof_of_equivalence",
            "aggregate": aggregate,
            "native": {},
            "sgx": {
                "instance_id": 1234,
                "setup": False,
                "bootstrap": False,
                "prove": True,
            },
            "risc0": {
                "bonsai": False,
                "snark": True,
                "profile": True,
                "execution_po2": 20,
            },
            "sp1": {"recursion": "plonk", "prover": "network", "verify": True},
        }

    async def submit_to_raiko(
        self, proposal_id: int, l1_inclusion_block: int, l2_block_numbers: list[int], last_anchor_block_number: int
    ) -> Optional[str]:
        """submit batch to Raiko"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}

            proposal_data = {
                "proposal_id": proposal_id,
                "l1_inclusion_block_number": l1_inclusion_block,
                "l2_block_numbers": l2_block_numbers,
                "checkpoint": None,
                "last_anchor_block_number": last_anchor_block_number,
            }
            
            payload = self.generate_post_data([proposal_data], aggregate=False)
            print(f"payload = {payload}")

            response = requests.post(
                f"{self.raiko_rpc}/v3/proof/batch/shasta",
                headers=headers,
                json=payload,
                timeout=10,
            )
            result = response.json()
            if "data" in result:
                result["data"] = {}  # avoid big print
            if result.get("status") == "ok":
                self.logger.info(
                    f"Proposal {proposal_id} (L2 blocks {l2_block_numbers}) in L1 block {l1_inclusion_block} submitted to Raiko with response: {result}"
                )
                return None
            else:
                self.logger.error(
                    f"Failed to submit proposal: {result.get('message', 'Unknown error')}"
                )
                return None
        except Exception as e:
            self.logger.error(f"Failed to submit to Raiko: {e}")
            return None
    
    async def submit_aggregate_to_raiko(self) -> Optional[str]:
        """submit aggregate request to Raiko"""
        if len(self.pending_proposals) == 0:
            return None
            
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            
            # Use the collected proposals
            proposals_to_aggregate = self.pending_proposals.copy()
            self.pending_proposals.clear()
            
            payload = self.generate_post_data(proposals_to_aggregate, aggregate=True)
            proposal_ids = [p["proposal_id"] for p in proposals_to_aggregate]
            self.logger.info(
                f"Submitting aggregate request for {len(proposals_to_aggregate)} proposals: {proposal_ids}"
            )
            print(f"aggregate payload = {payload}")

            response = requests.post(
                f"{self.raiko_rpc}/v3/proof/batch/shasta",
                headers=headers,
                json=payload,
                timeout=10,
            )
            result = response.json()
            if "data" in result:
                result["data"] = {}  # avoid big print
            if result.get("status") == "ok":
                self.aggregate_running_count += 1
                self.aggregate_requests.append(proposals_to_aggregate)
                self.logger.info(
                    f"Aggregate request for proposals {proposal_ids} submitted to Raiko with response: {result}, "
                    f"current running aggregate requests: {self.aggregate_running_count}"
                )
                return None
            else:
                self.logger.error(
                    f"Failed to submit aggregate request: {result.get('message', 'Unknown error')}"
                )
                return None
        except Exception as e:
            self.logger.error(f"Failed to submit aggregate to Raiko: {e}")
            return None

    async def query_raiko_status(
        self, proposal_id: int, l1_inclusion_block: int, l2_block_numbers: list[int], last_anchor_block_number: int
    ) -> RaikoResponse:
        """query Raiko status"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            proposal_data = {
                "proposal_id": proposal_id,
                "l1_inclusion_block_number": l1_inclusion_block,
                "l2_block_numbers": l2_block_numbers,
                "checkpoint": None,
                "last_anchor_block_number": last_anchor_block_number,
            }
            payload = self.generate_post_data([proposal_data], aggregate=False)
            response = requests.post(
                f"{self.raiko_rpc}/v3/proof/batch/shasta",
                headers=headers,
                json=payload,
                timeout=10,
            )
            result = response.json()
            if result.get("status") == "error":
                return RaikoResponse(
                    status="error", message=result.get("message", "Unknown error")
                )
            elif result.get("status") == "ok":
                return RaikoResponse(status="ok", data=result.get("data", {}))
            else:
                return RaikoResponse(status="error", message="Invalid response format")
        except Exception as e:
            self.logger.error(f"Failed to query Raiko status: {e}")
            return RaikoResponse(status="error", message=str(e))
    
    async def query_aggregate_status(self, proposals: list[Dict[str, Any]]) -> RaikoResponse:
        """query aggregate request status"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            payload = self.generate_post_data(proposals, aggregate=True)
            response = requests.post(
                f"{self.raiko_rpc}/v3/proof/batch/shasta",
                headers=headers,
                json=payload,
                timeout=10,
            )
            result = response.json()
            if result.get("status") == "error":
                return RaikoResponse(
                    status="error", message=result.get("message", "Unknown error")
                )
            elif result.get("status") == "ok":
                return RaikoResponse(status="ok", data=result.get("data", {}))
            else:
                return RaikoResponse(status="error", message="Invalid response format")
        except Exception as e:
            self.logger.error(f"Failed to query aggregate status: {e}")
            return RaikoResponse(status="error", message=str(e))

    async def process_proposal_group(self, group: ProposalGroup, l1_inclusion_block: int):
        """handle new proposal group"""
        try:
            if self.watch_mode:
                self.logger.info(f"Watch mode, skip processing")
                return

            start_time = datetime.now()
            self.logger.info(
                f"Starting to process proposal {group.proposal_id} (L2 blocks {group.l2_block_numbers}) @ L1 block {l1_inclusion_block} at {start_time}"
            )

            with open(self.log_file, "a") as f:
                f.write(
                    f"\nproposal {group.proposal_id} (L2 blocks {group.l2_block_numbers}) in L1 block {l1_inclusion_block} processing started at {start_time}\n"
                )

            # Get last_anchor_block_number from parent of the first block in the proposal
            first_block = group.l2_block_numbers[0]
            last_anchor_block_number = 0  # Default to 0 if no parent block
            
            if first_block > 0:
                parent_block = first_block - 1
                parent_anchor_info = await self.parse_l2_block_anchor_tx(parent_block)
                if parent_anchor_info is not None:
                    last_anchor_block_number = parent_anchor_info.anchor_number
                    self.logger.info(
                        f"Found last_anchor_block_number={last_anchor_block_number} from parent block {parent_block}"
                    )
                else:
                    self.logger.warning(
                        f"Could not parse parent block {parent_block}, using default last_anchor_block_number=0"
                    )
            else:
                self.logger.info(
                    f"First block is 0, no parent block available, using default last_anchor_block_number=0"
                )

            # request Raiko
            await self.submit_to_raiko(group.proposal_id, l1_inclusion_block, group.l2_block_numbers, last_anchor_block_number)

            # polling
            retry_count = 0
            start_polling_time = time.time()

            while True:
                if time.time() - start_polling_time > self.timeout:
                    self.logger.error(
                        f"Timeout waiting for proposal {group.proposal_id} / L1 block {l1_inclusion_block}"
                    )
                    break

                response = await self.query_raiko_status(group.proposal_id, l1_inclusion_block, group.l2_block_numbers, last_anchor_block_number)

                if response.status == "error":
                    self.logger.error(
                        f"Error processing proposal {group.proposal_id} in L1 block {l1_inclusion_block}: {response.message}"
                    )
                    retry_count += 1
                    if retry_count >= self.max_retries:
                        self.logger.error(
                            f"Max retries reached for proposal {group.proposal_id} in L1 block {l1_inclusion_block}"
                        )
                        break

                if response.data:
                    retry_count = 0  # reset retry count
                    if response.data.get("status") == "registered":
                        self.logger.info(
                            f"Proposal {group.proposal_id} in L1 Block {l1_inclusion_block} registered"
                        )
                    elif response.data.get("status") == "work_in_progress":
                        self.logger.info(
                            f"Proposal {group.proposal_id} in L1 Block {l1_inclusion_block} in progress"
                        )
                    elif response.data.get("proof"):
                        self.logger.info(
                            f"Proposal {group.proposal_id} in L1 Block {l1_inclusion_block} completed with proof {response.data['proof']['proof']}"
                        )
                        # If aggregate mode is enabled, add completed proposal to pending list
                        if self.aggregate > 0:
                            proposal_data = {
                                "proposal_id": group.proposal_id,
                                "l1_inclusion_block_number": l1_inclusion_block,
                                "l2_block_numbers": group.l2_block_numbers,
                                "checkpoint": None,
                                "last_anchor_block_number": last_anchor_block_number,
                            }
                            self.pending_proposals.append(proposal_data)
                            self.logger.info(
                                f"Added completed proposal {group.proposal_id} to pending aggregate list ({len(self.pending_proposals)}/{self.aggregate})"
                            )
                            
                            # If we've collected enough proposals, submit aggregate request
                            if len(self.pending_proposals) >= self.aggregate:
                                await self.submit_aggregate_to_raiko()
                        break
                    else:
                        self.logger.warning(
                            f"Proposal {group.proposal_id} in L1 Block {l1_inclusion_block} unhandled status: {response}"
                        )

                await asyncio.sleep(self.task_polling_interval)

            end_time = datetime.now()
            duration = (end_time - start_time).total_seconds()

            # log ending status
            with open(self.log_file, "a") as f:
                f.write(
                    f"Proposal {group.proposal_id} in L1 {l1_inclusion_block} processed {response.status} at {end_time}, duration: {duration} seconds\n"
                )
                if response.message:
                    f.write(f"Message: {response.message}\n")
                if response.data and response.data.get("proof"):
                    f.write(f"Proof: {response.data['proof']['proof']}\n")
        finally:
            self.running_count -= 1
            self.logger.info(
                f"Proposal {group.proposal_id} in L1 {l1_inclusion_block} processed, remaining running: {self.running_count}"
            )

    async def find_proposal_start_block(self, start_block: int, proposal_id: int) -> int:
        """
        Find the true start block of a proposal by checking backwards.
        Returns the first block number that belongs to this proposal_id.
        """
        current_block = start_block - 1
        last_valid_block = start_block
        
        while current_block >= 0:
            self.logger.debug(f"Checking backwards: block {current_block} for proposal {proposal_id}")
            anchor_info = await self.parse_l2_block_anchor_tx(current_block)
            
            if anchor_info is None:
                # Can't parse, stop going backwards
                self.logger.debug(f"Could not parse block {current_block}, stopping backwards search")
                break
            
            if anchor_info.proposal_id == proposal_id:
                # Same proposal, continue backwards
                last_valid_block = current_block
                current_block -= 1
            else:
                # Different proposal, we found the start
                self.logger.debug(f"Found different proposal {anchor_info.proposal_id} at block {current_block}, start is {last_valid_block}")
                break
        
        return last_valid_block

    async def find_proposal_end_block(self, end_block: int, proposal_id: int, max_forward: int = 100) -> int:
        """
        Find the true end block of a proposal by checking forwards.
        Returns the last block number that belongs to this proposal_id.
        max_forward limits how far forward we'll search to avoid infinite loops.
        """
        current_block = end_block + 1
        last_valid_block = end_block
        checked = 0
        
        while checked < max_forward:
            self.logger.debug(f"Checking forwards: block {current_block} for proposal {proposal_id}")
            anchor_info = await self.parse_l2_block_anchor_tx(current_block)
            
            if anchor_info is None:
                # Can't parse, stop going forwards
                self.logger.debug(f"Could not parse block {current_block}, stopping forwards search")
                break
            
            if anchor_info.proposal_id == proposal_id:
                # Same proposal, continue forwards
                last_valid_block = current_block
                current_block += 1
                checked += 1
            else:
                # Different proposal, we found the end
                self.logger.debug(f"Found different proposal {anchor_info.proposal_id} at block {current_block}, end is {last_valid_block}")
                break
        
        return last_valid_block

    async def process_l2_block_range(self):
        """
        Process L2 block range:
        1. Parse each L2 block to get anchor tx info (proposal_id, anchor_number)
        2. Find true start point if start block is in middle of a proposal
        3. Group consecutive blocks by proposal_id
        4. For each group, find L1 inclusion block
        5. Submit to Raiko
        """
        if self.l2_block_range is None:
            self.logger.error("L2 block range is required")
            return
        
        start_l2_block, end_l2_block = self.l2_block_range
        self.logger.info(f"Processing L2 blocks from {start_l2_block} to {end_l2_block}")
        
        # Step 1: Parse the first block to check if we're in the middle of a proposal
        first_block_info = await self.parse_l2_block_anchor_tx(start_l2_block)
        if first_block_info is None:
            self.logger.error(f"Failed to parse first block {start_l2_block}, cannot proceed")
            return
        
        # Step 2: Check if we need to go backwards to find the true start
        true_start_block = start_l2_block
        if start_l2_block > 0:
            # Check parent block to see if it has the same proposal_id
            parent_info = await self.parse_l2_block_anchor_tx(start_l2_block - 1)
            if parent_info is not None and parent_info.proposal_id == first_block_info.proposal_id:
                # We're in the middle of a proposal, find the true start
                self.logger.info(
                    f"Start block {start_l2_block} is in the middle of proposal {first_block_info.proposal_id}, "
                    f"finding true start..."
                )
                true_start_block = await self.find_proposal_start_block(start_l2_block, first_block_info.proposal_id)
                self.logger.info(f"True start block for proposal {first_block_info.proposal_id} is {true_start_block}")
        
        # Step 3: Parse the last block to check if we're in the middle of a proposal
        last_block_info = await self.parse_l2_block_anchor_tx(end_l2_block)
        true_end_block = end_l2_block
        if last_block_info is not None:
            # Check next block to see if it has the same proposal_id
            next_block_info = await self.parse_l2_block_anchor_tx(end_l2_block + 1)
            if next_block_info is not None and next_block_info.proposal_id == last_block_info.proposal_id:
                # We're in the middle of a proposal, find the true end
                self.logger.info(
                    f"End block {end_l2_block} is in the middle of proposal {last_block_info.proposal_id}, "
                    f"finding true end..."
                )
                true_end_block = await self.find_proposal_end_block(end_l2_block, last_block_info.proposal_id)
                self.logger.info(f"True end block for proposal {last_block_info.proposal_id} is {true_end_block}")
        
        # Step 4: Parse all L2 blocks from true start to true end
        anchor_infos = []
        parse_start = true_start_block
        parse_end = true_end_block
        
        self.logger.info(f"Parsing L2 blocks from {parse_start} to {parse_end}")
        for l2_block_num in range(parse_start, parse_end + 1):
            self.logger.info(f"Parsing L2 block {l2_block_num}")
            anchor_info = await self.parse_l2_block_anchor_tx(l2_block_num)
            if anchor_info is None:
                self.logger.warning(f"Failed to parse anchor tx from L2 block {l2_block_num}, skipping")
                continue
            anchor_infos.append(anchor_info)
            self.logger.info(
                f"L2 block {l2_block_num}: proposal_id={anchor_info.proposal_id}, anchor_number={anchor_info.anchor_number}"
            )
        
        if not anchor_infos:
            self.logger.error("No valid anchor transactions found in L2 block range")
            return
        
        # Step 2: Group consecutive blocks by proposal_id
        groups = self.group_blocks_by_proposal_id(anchor_infos)
        self.logger.info(f"Found {len(groups)} proposal groups")
        for group in groups:
            self.logger.info(
                f"Proposal {group.proposal_id}: anchor_number={group.anchor_number}, L2 blocks={group.l2_block_numbers}"
            )
        
        # Step 2.5: Batch query all proposal IDs (both normal and bond proposals)
        # Collect all proposal queries: (proposal_id, anchor_number)
        proposal_queries = []
        bond_proposal_queries = []
        
        for group in groups:
            # Normal proposal
            proposal_queries.append((group.proposal_id, group.anchor_number))
            
            # Bond proposal (proposal_id - 6)
            bond_proposal_id = group.proposal_id - 6
            if bond_proposal_id > 0:
                # For bond proposals, we don't have anchor_number, so use a wider search range
                # We'll estimate based on the normal proposal's anchor_number
                bond_proposal_queries.append((bond_proposal_id, group.anchor_number))
        
        # Determine search range based on anchor numbers
        if proposal_queries:
            min_anchor = min(anchor_number for _, anchor_number in proposal_queries)
            max_anchor = max(anchor_number for _, anchor_number in proposal_queries)
            
            # Normal proposals: search from min_anchor+1 to max_anchor+96
            # Bond proposals: search from min_anchor-200 to max_anchor+96 (wider range)
            normal_search_start = min_anchor + 1
            normal_search_end = max_anchor + 96
            
            bond_search_start = max(1, min_anchor - 200)
            bond_search_end = max_anchor + 96
            
            # Batch query normal proposals
            if proposal_queries:
                self.logger.info(
                    f"Batch querying {len(proposal_queries)} normal proposals in L1 blocks {normal_search_start} to {normal_search_end}"
                )
                await self.batch_find_proposal_blocks(proposal_queries, normal_search_start, normal_search_end)
            
            # Batch query bond proposals
            if bond_proposal_queries:
                self.logger.info(
                    f"Batch querying {len(bond_proposal_queries)} bond proposals in L1 blocks {bond_search_start} to {bond_search_end}"
                )
                await self.batch_find_proposal_blocks(bond_proposal_queries, bond_search_start, bond_search_end)
        
        # Step 3: For each group, find L1 inclusion block and submit
        acc_odds = 1
        tasks = []
        first_l1_block = None
        first_l1_timestamp = None
        
        for group in groups:
            self.logger.info(
                f"Processing proposal group {group.proposal_id} with L2 blocks {group.l2_block_numbers}"
            )
            
            # Find L1 inclusion block
            l1_inclusion_block = await self.find_l1_inclusion_block(
                group.proposal_id, group.anchor_number
            )
            
            if l1_inclusion_block is None:
                self.logger.warning(
                    f"Could not find L1 inclusion block for proposal {group.proposal_id}, skipping"
                )
                continue
            
            self.logger.info(
                f"Found L1 inclusion block {l1_inclusion_block} for proposal {group.proposal_id}"
            )
            
            # Get L1 block timestamp for time-based throttling
            l1_block = await self.get_block(l1_inclusion_block)
            if l1_block is None:
                self.logger.warning(f"Could not get L1 block {l1_inclusion_block}, skipping time check")
                l1_timestamp = None
            else:
                l1_timestamp = int(l1_block["timestamp"], 16)
            
            # Initialize time offset with first L1 block
            if first_l1_block is None and l1_timestamp is not None:
                first_l1_block = l1_inclusion_block
                first_l1_timestamp = l1_timestamp
                current_timestamp = int(time.time())
                self.ts_offset = current_timestamp - l1_timestamp
                self.last_block_ts_in_real_world = current_timestamp
                self.logger.info(
                    f"Initialized time offset: L1 block {first_l1_block} timestamp={l1_timestamp}, "
                    f"offset={self.ts_offset}, time_speed={self.time_speed}"
                )
            
            # Apply time-based throttling if time_speed is set and we have timestamps
            if self.time_speed > 0 and l1_timestamp is not None and self.ts_offset is not None:
                current_block_ts_in_real_world = l1_timestamp + self.ts_offset
                real_world_ts = int(time.time())
                real_world_elapsed_time = real_world_ts - self.last_block_ts_in_real_world
                accel_elapsed_time = real_world_elapsed_time * self.time_speed
                current_accel_ts = self.last_block_ts_in_real_world + accel_elapsed_time
                
                if current_accel_ts < current_block_ts_in_real_world:
                    # Need to wait before submitting this proposal
                    wait_time = (current_block_ts_in_real_world - current_accel_ts) / self.time_speed
                    self.logger.info(
                        f"L1 block {l1_inclusion_block} timestamp {current_block_ts_in_real_world} not reached yet, "
                        f"current accel time: {current_accel_ts}, waiting {wait_time:.2f} seconds..."
                    )
                    await asyncio.sleep(wait_time)
                    # Update after waiting
                    real_world_ts = int(time.time())
                    real_world_elapsed_time = real_world_ts - self.last_block_ts_in_real_world
                    accel_elapsed_time = real_world_elapsed_time * self.time_speed
                    current_accel_ts = self.last_block_ts_in_real_world + accel_elapsed_time
                
                # Update timestamp tracking
                self.last_block_ts_in_real_world = real_world_ts
                self.ts_offset = real_world_ts - l1_timestamp
            
            # Apply block_running_ratio
            acc_odds += self.block_running_ratio
            if acc_odds >= 1.0:
                aggregate_info = ""
                if self.aggregate > 0:
                    aggregate_info = f", aggregate running: {self.aggregate_running_count}"
                self.logger.info(
                    f"Submitting proposal {group.proposal_id} (L2 blocks {group.l2_block_numbers}) @ L1 block {l1_inclusion_block}, current running tasks: {self.running_count}{aggregate_info}"
                )
                self.running_count += 1
                acc_odds -= 1.0
                # Create task and add to list
                task = asyncio.create_task(
                    self.process_proposal_group(group, l1_inclusion_block)
                )
                tasks.append(task)
                # Yield control to event loop so task can start executing
                await asyncio.sleep(0)
            else:
                self.logger.info(
                    f"Proposal {group.proposal_id} skipped due to block_running_ratio:{self.block_running_ratio}"
                )
        
        # Wait for all tasks to complete
        self.logger.info(f"Waiting for {len(tasks)} processing tasks to complete...")
        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)
        
        # Submit any remaining pending proposals as aggregate if aggregate mode is enabled
        if self.aggregate > 0 and len(self.pending_proposals) > 0:
            self.logger.info(
                f"Submitting remaining {len(self.pending_proposals)} proposals as aggregate"
            )
            await self.submit_aggregate_to_raiko()
        
        # Check aggregate requests status and decrease count when completed
        if self.aggregate > 0 and len(self.aggregate_requests) > 0:
            completed_requests = []
            for proposals in self.aggregate_requests:
                response = await self.query_aggregate_status(proposals)
                if response.data and response.data.get("proof"):
                    proposal_ids = [p["proposal_id"] for p in proposals]
                    self.logger.info(
                        f"Aggregate request for proposals {proposal_ids} completed"
                    )
                    completed_requests.append(proposals)
                    self.aggregate_running_count = max(0, self.aggregate_running_count - 1)
            
            # Remove completed requests
            for completed in completed_requests:
                self.aggregate_requests.remove(completed)
        
        self.logger.info("All L2 blocks processed")

    async def run(self):
        """main loop"""
        self.logger.info("Starting block monitor")
        # print start config
        config_dict = {
            "l1_rpc": self.l1_rpc,
            "l2_rpc": self.l2_rpc,
            "raiko_rpc": self.raiko_rpc,
            "l2_block_range": self.l2_block_range,
            "prove_type": self.prove_type,
            "block_running_ratio": self.block_running_ratio,
            "aggregate": self.aggregate,
        }
        self.logger.info(f"Config:\n{json.dumps(config_dict, indent=2, default=str)}")
        
        if self.l2_block_range is not None:
            # Process L2 block range
            await self.process_l2_block_range()
        else:
            self.logger.error("L2 block range is required for the new workflow")
            return


def parse_none_value(value, convert_func=str):
    if value and isinstance(value, str) and value.lower() in ["none", "null"]:
        return None
    return convert_func(value) if value is not None else None


def parse_block_range(value):
    """support block range in format start,end or none"""
    if value and value.lower() in ["none", "null"]:
        return None
    try:
        start_block, end_block = map(int, value.split(","))
        return (start_block, end_block)
    except (ValueError, AttributeError):
        raise argparse.ArgumentTypeError(
            'Block range must be in format "start,end" or "none"'
        )


async def main():
    parser = argparse.ArgumentParser(description="Block Monitor CLI")

    parser.add_argument(
        "-e",
        "--l1-rpc",
        type=lambda x: parse_none_value(x, str),
        default="https://l1rpc.internal.taiko.xyz",
        help='L1 Ethereum RPC endpoint (use "none" for None value)',
    )

    parser.add_argument(
        "-l",
        "--l2-rpc",
        type=lambda x: parse_none_value(x, str),
        default="https://l2rpc.internal.taiko.xyz",
        help='L2 RPC endpoint (use "none" for None value)',
    )

    parser.add_argument(
        "-a",
        "--raiko-rpc",
        type=lambda x: parse_none_value(x, str),
        default="http://localhost:8080",
        help='Raiko RPC endpoint (use "none" for None value)',
    )

    parser.add_argument(
        "-o",
        "--log-file",
        type=lambda x: parse_none_value(x, str),
        default="block_monitor.log",
        help='Log file path (use "none" for None value)',
    )

    parser.add_argument(
        "-p",
        "--polling-interval",
        type=lambda x: parse_none_value(x, int),
        default=3,
        help='Polling interval in seconds (use "none" for None value)',
    )

    parser.add_argument(
        "-r",
        "--block-running-ratio",
        type=lambda x: parse_none_value(x, float),
        default=1.0,
        help='Block running ratio (use "none" for None value)',
    )

    parser.add_argument(
        "-g",
        "--l2-block-range",
        type=parse_block_range,
        default="None",
        help='L2 block range in format "start,end" or "none" for None value',
    )

    parser.add_argument(
        "-t",
        "--prove-type",
        type=lambda x: parse_none_value(x, str),
        default="native",
        help='Prove type (use "none" for None value)',
    )

    parser.add_argument(
        "-i",
        "--abi-file",
        type=lambda x: parse_none_value(x, str),
        default="./IInbox.json",
        help='L1 event ABI file path (use "none" for None value)',
    )

    parser.add_argument(
        "-b",
        "--anchor-abi-file",
        type=lambda x: parse_none_value(x, str),
        default=None,
        help='L2 anchor contract ABI file path for decoding anchorV4 function (optional, use "none" for None value)',
    )

    parser.add_argument(
        "-c",
        "--event-contract",
        type=lambda x: parse_none_value(x, str),
        default="0x3b37a799290950fef954dfF547608baC52A12571",
        help='Event contract address (use "none" for None value)',
    )

    parser.add_argument(
        "-w",
        "--watch-event",
        action="store_true",
        default=False,
        help='Watch proposal event only (use "none" for None value)',
    )

    parser.add_argument(
        "-x",
        "--time-speed",
        type=lambda x: parse_none_value(x, float),
        default=1.0,
        help="time scaling, real world 1s to 1*x s in stress",
    )

    parser.add_argument(
        "-A",
        "--aggregate",
        type=lambda x: parse_none_value(x, int),
        default=0,
        help="Aggregate mode: if > 0, collect n proposals and submit as aggregate request",
    )

    args = parser.parse_args()

    monitor = BatchMonitor(
        l1_rpc=args.l1_rpc,
        l2_rpc=args.l2_rpc,
        abi_file=args.abi_file,
        evt_address=web3.Web3.to_checksum_address(args.event_contract),
        raiko_rpc=args.raiko_rpc,
        log_file=args.log_file,
        polling_interval=args.polling_interval,
        block_running_ratio=args.block_running_ratio,
        l2_block_range=args.l2_block_range,
        prove_type=args.prove_type,
        watch_mode=args.watch_event,
        time_speed=args.time_speed,
        anchor_abi_file=args.anchor_abi_file,
        aggregate=args.aggregate,
    )

    await monitor.run()


# Example usage:
# python stress_shasta_proposal.py -t native -g 1000,2000 -p 10 -o stress_dev.log -a http://localhost:8080 -c 0xe9BDA5fd0C7F8E97b12860a57Cbcc89f33AAfFE8 -e https://l1rpc.internal.taiko.xyz -l https://l2rpc.internal.taiko.xyz -i IInbox.json -b Anchor.json -x 100 -w
# python stress_shasta_proposal.py -t native -g 1000,2000 -A 5 -a http://localhost:8080 -c 0xe9BDA5fd0C7F8E97b12860a57Cbcc89f33AAfFE8 -e https://l1rpc.internal.taiko.xyz -l https://l2rpc.internal.taiko.xyz -i IInbox.json -b Anchor.json
# Note: 
#   -a (--raiko-rpc): Raiko RPC endpoint (default: http://localhost:8080)
#   -g now specifies L2 block range instead of L1 block range
#   -b (--anchor-abi-file) is optional but recommended for proper anchorV4 decoding
#   -A (--aggregate): Aggregate mode, if > 0, collect n proposals and submit as aggregate request
if __name__ == "__main__":
    asyncio.run(main())
