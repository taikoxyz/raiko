#!/usr/bin/env python3
"""
Test script to verify that aggregation requests get the highest priority.

Usage Examples:
  # Basic continuous streaming test with default settings (native prover)
  python3 test_aggregation_priority.py
  
  # Test with different prover types
  python3 test_aggregation_priority.py --prove-type sgx
  python3 test_aggregation_priority.py --prove-type risc0
  python3 test_aggregation_priority.py --prove-type sp1
  
"""

import asyncio
import json
import time
import logging
from typing import List, Dict, Any, Optional, Tuple
from datetime import datetime
import argparse
import requests
from dataclasses import dataclass
import hashlib
import random
import web3
from web3 import Web3
from web3.middleware import ExtraDataToPOAMiddleware


@dataclass
class RaikoResponse:
    status: str
    data: Optional[Dict[str, Any]] = None
    message: Optional[str] = None


@dataclass
class TestRequest:
    request_type: str  # "single_proof" or "aggregation"
    request_id: str
    submitted_time: float
    batch_ids: List[int]
    submission_success: bool = False
    sequence_number: int = 0  # Position in the streaming sequence


class AggregationPriorityTester:
    def __init__(
        self,
        raiko_rpc: str = "http://localhost:8080",
        l1_rpc: str = "http://35.240.156.196:8545",
        abi_file: str = "./script/ITaikoInbox.json",
        evt_address: str = "0x79C9109b764609df928d16fC4a91e9081F7e87DB",
        log_file: str = "aggregation_priority_test.log",
        prove_type: str = "native",
        request_delay: float = 1.0,
        polling_interval: int = 3,
        request_timeout: int = 90,
    ):
        self.raiko_rpc = raiko_rpc
        self.l1_rpc = l1_rpc
        self.abi_file = abi_file
        self.evt_address = evt_address
        self.log_file = log_file
        self.prove_type = prove_type
        self.request_delay = request_delay
        self.polling_interval = polling_interval
        self.request_timeout = request_timeout
        self.test_requests: List[TestRequest] = []
        
        self.test_id = hashlib.md5(f"{time.time()}_{random.randint(1000, 9999)}".encode()).hexdigest()[:8]
        
        # Setup logging
        logging.basicConfig(
            level=logging.INFO,
            format="%(asctime)s - %(levelname)s - %(message)s",
            handlers=[
                logging.FileHandler(log_file),
                logging.StreamHandler()
            ],
        )
        self.logger = logging.getLogger(__name__)
        
        self.logger.info(f"Starting test with ID: {self.test_id}")
        self.logger.info(f"Using prover type: {self.prove_type}")
        
        # Validate prover type
        supported_provers = ["native", "sgx", "risc0", "sp1"]
        if self.prove_type not in supported_provers:
            self.logger.error(f"Unsupported prover type: {self.prove_type}")
            self.logger.error(f"Supported provers: {supported_provers}")
            raise ValueError(f"Unsupported prover type: {self.prove_type}")
        
        # Initialize L1 chain connection
        self.__init_contract_event()

    def __init_contract_event(self):
        """Initialize L1 chain connection and contract event monitoring"""
        try:
            with open(self.abi_file) as f:
                abi = json.load(f)
            l1_w3 = Web3(Web3.HTTPProvider(self.l1_rpc, {"timeout": 10}))
            l1_w3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)
            if l1_w3.is_connected():
                self.logger.info(f"Connected to L1 node {self.l1_rpc}")
            else:
                self.logger.error(f"Failed to connect to L1 node {self.l1_rpc}")
                self.evt_contract = None
                return
            self.evt_contract = l1_w3.eth.contract(address=self.evt_address, abi=abi["abi"])
            self.logger.info(f"Initialized contract event monitoring for {self.evt_address}")
        except Exception as e:
            self.logger.error(f"Failed to initialize contract event monitoring: {e}")
            self.evt_contract = None

    def get_batch_events_in_block(self, block_number: int) -> List[Tuple[int, int]]:
        """Get batch IDs and L1 inclusion blocks from BatchProposed events in a specific block"""
        if not hasattr(self, 'evt_contract') or self.evt_contract is None:
            self.logger.error("Contract not initialized, cannot get batch events")
            return []
        
        try:
            logs = self.evt_contract.events.BatchProposed().get_logs(
                from_block=block_number, to_block=block_number
            )
            return [(log.args.meta.batchId, block_number) for log in logs]
        except Exception as e:
            self.logger.error(f"Failed to get batch events in block {block_number}: {e}")
            return []

    def get_available_batch_ids(self, start_block: int, end_block: int, max_batches: int = 5000) -> List[Tuple[int, int]]:
        """Get available batch IDs and L1 inclusion blocks from L1 chain within a block range"""
        if not hasattr(self, 'evt_contract') or self.evt_contract is None:
            self.logger.error("Contract not initialized, cannot get batch events")
            return []
        
        batch_data = []
        current_block = start_block
        
        self.logger.info(f"Scanning blocks {start_block} to {end_block} for BatchProposed events...")
        
        while current_block <= end_block and len(batch_data) < max_batches:
            batch_events = self.get_batch_events_in_block(current_block)
            for batch_id, l1_inclusion_block in batch_events:
                batch_data.append((batch_id, l1_inclusion_block))
                if len(batch_data) % 100 == 0:
                    self.logger.info(f"Found {len(batch_data)} batch proposals so far...")
                if len(batch_data) >= max_batches:
                    break
            current_block += 1
        
        if len(batch_data) == 0:
            self.logger.error(f"No batch proposals found in blocks {start_block}-{end_block}")
            return []
        
        self.logger.info(f"Found {len(batch_data)} batch proposals")
        return batch_data

    def create_single_proof_request(self, batch_id: int, l1_inclusion_block: int, aggregate: bool = False) -> Dict[str, Any]:
        """Create a single proof request payload"""
        base_request = {
            "batches": [
                {
                    "batch_id": batch_id,
                    "l1_inclusion_block_number": l1_inclusion_block,
                }
            ],
            "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
            "graffiti": "8008500000000000000000000000000000000000000000000000000000000000",
            "proof_type": self.prove_type,
            "blob_proof_type": "proof_of_equivalence",
            "aggregate": aggregate,
        }
        
        if self.prove_type == "native":
            base_request["native"] = {}
        elif self.prove_type == "sgx":
            base_request["sgx"] = {
                "instance_id": 1234,
                "setup": False,
                "bootstrap": False,
                "prove": True,
            }
        elif self.prove_type == "risc0":
            base_request["risc0"] = {
                "bonsai": True,
                "snark": True,
                "profile": False,
                "execution_po2": 20,
            }
        elif self.prove_type == "sp1":
            base_request["sp1"] = {
                "recursion": "plonk",
                "prover": "network",
                "verify": True
            }
        
        return base_request

    def create_aggregation_request(self, batch_data: List[Tuple[int, int]]) -> Dict[str, Any]:
        """Create an aggregation request payload"""
        base_request = {
            "batches": [
                {
                    "batch_id": batch_id,
                    "l1_inclusion_block_number": l1_inclusion_block,
                }
                for batch_id, l1_inclusion_block in batch_data
            ],
            "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
            "graffiti": "8008500000000000000000000000000000000000000000000000000000000000",
            "proof_type": self.prove_type,
            "blob_proof_type": "proof_of_equivalence",
            "aggregate": True,
        }

        if self.prove_type == "native":
            base_request["native"] = {}
        elif self.prove_type == "sgx":
            base_request["sgx"] = {
                "instance_id": 1234,
                "setup": False,
                "bootstrap": False,
                "prove": True,
            }
        elif self.prove_type == "risc0":
            base_request["risc0"] = {
                "bonsai": True,
                "snark": True,
                "profile": False,
                "execution_po2": 20,
            }
        elif self.prove_type == "sp1":
            base_request["sp1"] = {
                "recursion": "plonk",
                "prover": "network",
                "verify": True
            }
        
        return base_request

    async def submit_request(self, payload: Dict[str, Any], request_type: str, request_id: str, sequence_number: int) -> Optional[Dict[str, Any]]:
        """Submit a request to the Raiko server"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            endpoint = f"{self.raiko_rpc}/v3/proof/batch"
            
            self.logger.info(f"Submitting {request_type} request {request_id} (sequence #{sequence_number})")
            start_time = time.time()
            response = requests.post(
                endpoint,
                headers=headers,
                json=payload,
                timeout=30,
            )
            
            try:
                result = response.json()
            except json.JSONDecodeError:
                return None

            # Extract batch IDs from the payload
            if request_type == "single_proof":
                batch_ids = [payload["batches"][0]["batch_id"]]
            else:
                batch_ids = [batch["batch_id"] for batch in payload["batches"]]
            
            # Create test request record
            test_request = TestRequest(
                request_type=request_type,
                request_id=request_id,
                submitted_time=start_time,
                batch_ids=batch_ids,
                submission_success=response.status_code == 200,
                sequence_number=sequence_number
            )
            self.test_requests.append(test_request)
            
            if response.status_code == 200:
                self.logger.info(f"Successfully submitted {request_type} request {request_id} (sequence #{sequence_number})")
                return {
                    "request_type": request_type,
                    "request_id": request_id,
                    "payload": payload,
                    "batch_ids": batch_ids,
                    "response": result,
                    "sequence_number": sequence_number
                }
            else:
                return None
                
        except requests.RequestException as e:
            return None
        except Exception as e:
            return None

    async def raiko_status_query(self, payload: Dict[str, Any], request_type: str, request_id: str) -> RaikoResponse:
        """Query the status of a request"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            endpoint = f"{self.raiko_rpc}/v3/proof/batch"
            
            response = requests.post(
                endpoint,
                headers=headers,
                json=payload,
                timeout=self.request_timeout,
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
        except requests.exceptions.Timeout as e:
            self.logger.warning(f"Timeout querying Raiko status for {request_type} request {request_id}: {e}")
            return RaikoResponse(status="error", message=f"Request timeout: {str(e)}")
        except requests.exceptions.ConnectionError as e:
            self.logger.warning(f"Connection error querying Raiko status for {request_type} request {request_id}: {e}")
            return RaikoResponse(status="error", message=f"Connection error: {str(e)}")
        except requests.exceptions.RequestException as e:
            self.logger.warning(f"Request error querying Raiko status for {request_type} request {request_id}: {e}")
            return RaikoResponse(status="error", message=f"Request error: {str(e)}")
        except Exception as e:
            self.logger.error(f"Unexpected error querying Raiko status for {request_type} request {request_id}: {e}")
            return RaikoResponse(status="error", message=str(e))

    async def submit_aggregation_after_successful_proofs(self, batch_data: List[Tuple[int, int]], 
                                                        max_wait_time: int = 3600) -> List[Dict[str, Any]]:
        self.logger.info("="*60)
        self.logger.info("STARTING CONTINUOUS STREAMING AGGREGATION TEST")
        self.logger.info("="*60)
        
        successful_aggregations = []
        successful_single_proofs = []
        current_batch_group = []
        submitted_requests = {}  # Track submitted requests by request_id
        completed_proofs = set()  # Track completed proofs by batch_id
        
        # Start submitting single proof requests continuously
        submission_task = asyncio.create_task(self._submit_continuous_single_proofs(
            batch_data, submitted_requests
        ))
        
        # Start monitoring for completed proofs
        monitoring_task = asyncio.create_task(self._monitor_completed_proofs(
            submitted_requests, completed_proofs, current_batch_group, 
            successful_single_proofs, successful_aggregations, max_wait_time
        ))
        
        try:
            # Wait for both tasks to complete
            await asyncio.gather(submission_task, monitoring_task)
        except asyncio.CancelledError:
            self.logger.info("Tasks cancelled")
        except Exception as e:
            self.logger.error(f"Error in continuous streaming: {e}")
        
        self.logger.info(f"Final Summary: {len(successful_single_proofs)} successful single proofs, {len(successful_aggregations)} aggregation requests submitted")
        return successful_aggregations

    async def _submit_continuous_single_proofs(self, batch_data: List[Tuple[int, int]], 
                                             submitted_requests: Dict[str, Dict]) -> None:
        """Continuously submit single proof requests"""
        self.logger.info("Starting continuous single proof submission...")
        
        for i, (batch_id, l1_inclusion_block) in enumerate(batch_data):
            # Create and submit single proof request
            payload = self.create_single_proof_request(batch_id, l1_inclusion_block, aggregate=False)
            request_id = f"single_{batch_id}_{self.test_id}"
            request_type = "single_proof"
            
            # Submit the request
            submission_result = await self.submit_request(payload, request_type, request_id, i+1)
            if submission_result:
                self.logger.info(f"Submitted single proof request for batch {batch_id} (sequence {i+1})")
                submitted_requests[request_id] = {
                    "batch_id": batch_id,
                    "l1_inclusion_block": l1_inclusion_block,
                    "payload": payload,
                    "request_type": request_type,
                    "sequence": i+1,
                    "submitted_time": time.time()
                }
            else:
                self.logger.error(f"Failed to submit single proof request for batch {batch_id}")
            
            # Small delay between submissions
            await asyncio.sleep(self.request_delay)
        
        self.logger.info(f"Finished submitting {len(batch_data)} single proof requests")

    async def _monitor_completed_proofs(self, submitted_requests: Dict[str, Dict], 
                                      completed_proofs: set, current_batch_group: List[Tuple[int, int]],
                                      successful_single_proofs: List[Dict], 
                                      successful_aggregations: List[Dict],
                                      max_wait_time: int) -> None:
        self.logger.info("Starting monitoring for completed proofs...")
        
        start_time = time.time()
        aggregation_count = 0
        
        while time.time() - start_time < max_wait_time and len(completed_proofs) < len(submitted_requests):
            for request_id, request_info in submitted_requests.items():
                batch_id = request_info["batch_id"]
                
                # Skip if already completed
                if batch_id in completed_proofs:
                    continue
                
                # Check if proof is completed
                payload = request_info["payload"]
                request_type = request_info["request_type"]
                
                try:
                    response = await self.raiko_status_query(payload, request_type, request_id)
                    
                    if response.status == "ok" and response.data:
                        data = response.data
                        status = data.get("status", "unknown")
                        if data.get("proof"):
                            self.logger.info(f"Proof completed for batch {batch_id}!")
                            completed_proofs.add(batch_id)
                            
                            # Add to successful single proofs
                            successful_single_proofs.append({
                                "batch_id": batch_id,
                                "l1_inclusion_block": request_info["l1_inclusion_block"],
                                "request_id": request_id,
                                "sequence": request_info["sequence"]
                            })
                            
                            # Add to current batch group
                            current_batch_group.append((batch_id, request_info["l1_inclusion_block"]))
                            
                            if len(current_batch_group) == 2:
                                aggregation_count += 1
                                self.logger.info(f"2 successful proofs achieved! Submitting aggregation request #{aggregation_count} for batches: {[b for b, _ in current_batch_group]}")
                                
                                # Create aggregation payload
                                agg_payload = self.create_aggregation_request(current_batch_group)
                                agg_request_id = f"aggregation_{aggregation_count}_{self.test_id}"
                                agg_request_type = "aggregation"
                                
                                # Submit aggregation request
                                agg_submission = await self.submit_request(agg_payload, agg_request_type, agg_request_id, aggregation_count)
                                
                                if agg_submission:
                                    self.logger.info(f"Aggregation request #{aggregation_count} submitted successfully for batches: {[b for b, _ in current_batch_group]}")
                                    successful_aggregations.append({
                                        "request_id": agg_request_id,
                                        "batch_ids": [b for b, _ in current_batch_group],
                                        "submission": agg_submission,
                                        "aggregation_number": aggregation_count
                                    })
                                else:
                                    self.logger.error(f"Failed to submit aggregation request #{aggregation_count} for batches: {[b for b, _ in current_batch_group]}")
                                
                                # Reset batch group for next aggregation
                                current_batch_group = []
                            else:
                                self.logger.info(f"Progress: {len(current_batch_group)}/2 successful proofs for next aggregation")
                        
                        elif status in ["registered", "work_in_progress"]:
                            # Still in progress, continue monitoring
                            pass
                        elif status == "failed":
                            self.logger.error(f"Proof failed for batch {batch_id}")
                            completed_proofs.add(batch_id)  # Mark as completed to avoid re-checking
                
                except Exception as e:
                    self.logger.error(f"Error monitoring request {request_id}: {e}")
            
            # Check if we should continue monitoring
            if len(completed_proofs) >= len(submitted_requests):
                self.logger.info("All submitted requests have been processed")
                break
            
            # Wait before next monitoring cycle
            await asyncio.sleep(self.polling_interval)
        
        # Handle remaining successful proofs (if not exactly divisible by 2)
        if len(current_batch_group) > 0:
            self.logger.info(f"Remaining {len(current_batch_group)} successful proofs that don't form a complete group of 2")
        
        self.logger.info(f"Monitoring complete. Processed {len(completed_proofs)} proofs, submitted {len(successful_aggregations)} aggregation requests")

    async def monitor_aggregation_priority(self, single_proof_requests: List[Dict], aggregation_requests: List[Dict], 
                                         monitor_duration: int = 600) -> Dict[str, Any]:
        self.logger.info(f"Monitoring aggregation priority for {monitor_duration} seconds...")
        
        single_proof_status = {}
        aggregation_status = {}
        processing_order = []
        completed_aggregations = set()
        
        start_time = time.time()
        
        while time.time() - start_time < monitor_duration:
            current_time = time.time()
            
            # Monitor aggregation requests
            for req in aggregation_requests:
                request_id = req["request_id"]
                
                # Skip if already completed
                if request_id in completed_aggregations:
                    continue
                
                batch_ids = req["batch_ids"]
                payload = self.create_aggregation_request([(bid, 4110000) for bid in batch_ids])
                
                try:
                    response = await self.raiko_status_query(payload, "aggregation", request_id)
                    
                    if response.status == "ok" and response.data:
                        data = response.data
                        status = data.get("status", "unknown")
                        self.logger.info(f"Aggregation proof completed for {request_id}!")
                        completed_aggregations.add(request_id)
                        
                        # Record completion time for priority analysis
                        if request_id not in [p[1] for p in processing_order]:
                            processing_order.append(("aggregation", request_id, current_time, "completed"))
                        
                        if status == "failed":
                            self.logger.error(f"Aggregation failed for {request_id}")
                            completed_aggregations.add(request_id)  # Mark as completed to avoid re-checking
                    
                    elif response.status == "error":
                        # Handle timeout and connection errors more gracefully
                        if "timeout" in response.message.lower() or "connection" in response.message.lower():
                            self.logger.warning(f"Network issue querying aggregation {request_id}: {response.message}")
                            # Don't mark as completed for network issues, will retry
                        else:
                            self.logger.error(f"Error querying aggregation {request_id}: {response.message}")
                
                except Exception as e:
                    self.logger.error(f"Unexpected error monitoring aggregation request {request_id}: {e}")
            
            # Check if all aggregations are completed
            if len(completed_aggregations) >= len(aggregation_requests):
                self.logger.info("All aggregation requests have been processed")
                break
            
            await asyncio.sleep(self.polling_interval)
        
        # Analyze processing order
        aggregation_started = [p for p in processing_order if p[0] == "aggregation"]
        
        analysis = {
            "total_aggregations": len(aggregation_requests),
            "aggregations_started": len(aggregation_started),
            "aggregations_completed": len(completed_aggregations),
            "processing_order": processing_order,
            "priority_working": len(aggregation_started) > 0
        }
        
        if analysis["priority_working"]:
            # Check if aggregations are being processed
            analysis["aggregation_priority"] = len(aggregation_started) > 0
            analysis["priority_message"] = f"Aggregation requests are being processed. Started: {len(aggregation_started)}, Completed: {len(completed_aggregations)}"
        
        return analysis

    async def run_continuous_streaming_aggregation_test(self, total_batches: int = 60, base_block: int = 4110000, 
                                                      max_wait_time: int = 3600, monitor_duration: int = 600) -> bool:
        self.logger.info("="*60)
        self.logger.info("STARTING CONTINUOUS STREAMING AGGREGATION TEST")
        self.logger.info("="*60)
        self.logger.info(f"Test ID: {self.test_id}")
        self.logger.info(f"Will submit {total_batches} single proof requests continuously")
        
        if not hasattr(self, 'evt_contract') or self.evt_contract is None:
            self.logger.error("L1 contract not initialized - cannot proceed with test")
            return False
        
        self.logger.info(f"Getting {total_batches} batch IDs from L1 chain starting from block {base_block}")
        batch_data = self.get_available_batch_ids(base_block, base_block + 5000, total_batches)
        
        if len(batch_data) == 0:
            self.logger.error("No batch proposals found - cannot proceed with test")
            return False
        
        if len(batch_data) < total_batches:
            self.logger.error(f"Not enough batch proposals found. Need {total_batches}, got {len(batch_data)}")
            return False
        
        self.logger.info(f"Found {len(batch_data)} batch proposals to use for testing")
        
        successful_aggregations = await self.submit_aggregation_after_successful_proofs(
            batch_data[:total_batches], max_wait_time
        )
        
        if len(successful_aggregations) == 0:
            self.logger.error("No aggregation requests were submitted - cannot test priority")
            return False
        
        self.logger.info(f"Successfully submitted {len(successful_aggregations)} aggregation requests")
        
        # Monitor the processing priority
        self.logger.info("Starting to monitor aggregation priority...")
        analysis = await self.monitor_aggregation_priority(
            [], successful_aggregations, monitor_duration
        )
        
        return analysis.get("aggregation_priority", False)


async def main():
    parser = argparse.ArgumentParser(description="Test aggregation request priority with continuous streaming")
    
    parser.add_argument(
        "--raiko-rpc",
        type=str,
        default="http://localhost:8080",
        help="Raiko RPC endpoint"
    )
    
    parser.add_argument(
        "--l1-rpc",
        type=str,
        default="http://35.240.156.196:8545",
        help="L1 RPC endpoint"
    )
    
    parser.add_argument(
        "--abi-file",
        type=str,
        default="./script/ITaikoInbox.json",
        help="ABI file path"
    )
    
    parser.add_argument(
        "--event-contract",
        type=str,
        default="0x79C9109b764609df928d16fC4a91e9081F7e87DB",
        help="Event contract address"
    )
    
    parser.add_argument(
        "--log-file",
        type=str,
        default="aggregation_priority_test.log",
        help="Log file path"
    )
    
    parser.add_argument(
        "--total-batches",
        type=int,
        default=60,
        help="Total number of batches to process"
    )
    
    parser.add_argument(
        "--prove-type",
        type=str,
        default="native",
        choices=["native", "sgx", "risc0", "sp1"],
        help="Proof type to use for requests"
    )
    
    parser.add_argument(
        "--request-delay",
        type=float,
        default=2.0,
        help="Delay between submissions"
    )
    
    parser.add_argument(
        "--base-block",
        type=int,
        default=4110000,
        help="Base block number to start testing from"
    )
    
    parser.add_argument(
        "--monitor-duration",
        type=int,
        default=600,
        help="How long to monitor request processing (seconds)"
    )
    
    parser.add_argument(
        "--polling-interval",
        type=int,
        default=5,
        help="Polling interval for status queries (seconds)"
    )
    
    parser.add_argument(
        "--max-wait-time",
        type=int,
        default=3600,
        help="Maximum time to wait for proof completion (seconds)"
    )
    
    parser.add_argument(
        "--request-timeout",
        type=int,
        default=90,
        help="Timeout for individual HTTP requests (seconds)"
    )
    
    parser.add_argument(
        "--quick-test",
        action="store_true",
        help="Run a quick test with reduced delays and monitoring time"
    )
    
    args = parser.parse_args()
    
    tester = AggregationPriorityTester(
        raiko_rpc=args.raiko_rpc,
        l1_rpc=args.l1_rpc,
        abi_file=args.abi_file,
        evt_address=args.event_contract,
        log_file=args.log_file,
        prove_type=args.prove_type,
        request_delay=args.request_delay,
        polling_interval=args.polling_interval,
        request_timeout=args.request_timeout,
    )
    
    try:
        priority_working = await tester.run_continuous_streaming_aggregation_test(
            total_batches=args.total_batches,
            base_block=args.base_block,
            max_wait_time=args.max_wait_time,
            monitor_duration=args.monitor_duration
        )
        
        print("\n" + "="*60)
        if priority_working:
            exit(0)
        else:
            exit(1)
            
    except KeyboardInterrupt:
        print("\nTest interrupted by user")
        exit(130)
    except Exception as e:
        print(f"Test failed with exception: {e}")
        exit(1)


if __name__ == "__main__":
    asyncio.run(main()) 