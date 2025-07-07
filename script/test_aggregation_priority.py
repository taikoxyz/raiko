#!/usr/bin/env python3
"""
Test script to verify that aggregation requests get the highest priority.

This script tests the priority queue mechanism by:
1. Getting real batch IDs from L1 chain
2. Submitting single proof requests (low priority)
3. Submitting aggregation requests (high priority)
4. Monitoring that aggregation requests are processed with priority

Usage Examples:
  # Basic test with default settings (native prover)
  python3 test_aggregation_priority.py
  
  # Quick test with shorter delays
  python3 test_aggregation_priority.py --quick-test
  
  # Test with different prover types
  python3 test_aggregation_priority.py --prove-type sgx
  python3 test_aggregation_priority.py --prove-type risc0
  python3 test_aggregation_priority.py --prove-type sp1
  
  # Custom test with specific parameters
  python3 test_aggregation_priority.py --single-requests 10 --aggregation-requests 3 --prove-type sp1
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
class TestRequest:
    request_type: str  # "single_proof" or "aggregation"
    request_id: str
    submitted_time: float
    batch_ids: List[int]
    submission_success: bool = False


class AggregationPriorityTester:
    def __init__(
        self,
        raiko_rpc: str = "http://localhost:8080",
        l1_rpc: str = "https://ethereum-holesky-rpc.publicnode.com",
        abi_file: str = "./script/ITaikoInbox.json",
        evt_address: str = "0x79C9109b764609df928d16fC4a91e9081F7e87DB",
        log_file: str = "aggregation_priority_test.log",
        prove_type: str = "native",
        request_delay: float = 2.0,
        polling_interval: int = 200,
    ):
        self.raiko_rpc = raiko_rpc
        self.l1_rpc = l1_rpc
        self.abi_file = abi_file
        self.evt_address = evt_address
        self.log_file = log_file
        self.prove_type = prove_type
        self.request_delay = request_delay
        self.polling_interval = polling_interval
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

    def get_available_batch_ids(self, start_block: int, end_block: int, max_batches: int = 50) -> List[Tuple[int, int]]:
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
                self.logger.info(f"Found batch {batch_id} in L1 block {l1_inclusion_block}")
                if len(batch_data) >= max_batches:
                    break
            current_block += 1
        
        if len(batch_data) == 0:
            self.logger.error(f"No batch proposals found in blocks {start_block}-{end_block}")
            return []
        
        self.logger.info(f"Found {len(batch_data)} batch proposals")
        return batch_data

    def create_single_proof_request(self, batch_id: int, l1_inclusion_block: int) -> Dict[str, Any]:
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
        }
        
        # Add prover-specific configurations
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

    async def submit_request(self, payload: Dict[str, Any], request_type: str, request_id: str) -> Optional[Dict[str, Any]]:
        """Submit a request to the Raiko server"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            endpoint = f"{self.raiko_rpc}/v3/proof/batch"
            
            self.logger.info(f"Submitting {request_type} request {request_id}")
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
                self.logger.error(f"Invalid JSON response for {request_type} request {request_id}: {response.text}")
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
                submission_success=response.status_code == 200
            )
            self.test_requests.append(test_request)
            
            if response.status_code == 200:
                self.logger.info(f"Successfully submitted {request_type} request {request_id}")
                return {
                    "request_type": request_type,
                    "request_id": request_id,
                    "payload": payload,
                    "batch_ids": batch_ids,
                    "response": result
                }
            else:
                self.logger.error(f"Failed to submit {request_type} request {request_id}: HTTP {response.status_code} - {response.text}")
                return None
                
        except requests.RequestException as e:
            self.logger.error(f"Request exception submitting {request_type} request {request_id}: {e}")
            return None
        except Exception as e:
            self.logger.error(f"Unexpected exception submitting {request_type} request {request_id}: {e}")
            return None

    async def raiko_status_query(self, payload: Dict[str, Any], request_type: str, request_id: str) -> Dict[str, Any]:
        """Query the status of a request"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            endpoint = f"{self.raiko_rpc}/v3/proof/batch"
            
            response = requests.post(
                endpoint,
                headers=headers,
                json=payload,
                timeout=10,
            )
            result = response.json()
            
            if response.status_code != 200:
                return {"status": "error", "message": f"HTTP {response.status_code}: {response.text}"}
            
            return result
                
        except Exception as e:
            self.logger.error(f"Failed to query Raiko status for {request_type} request {request_id}: {e}")
            return {"status": "error", "message": str(e)}

    async def monitor_processing(self, submitted_requests: List[TestRequest], monitor_duration: int = 600):
        """Monitor the processing order of submitted requests"""
        self.logger.info(f"Monitoring request processing for {monitor_duration} seconds...")
        
        processing_order = []
        seen_tasks = set()
        task_status_history = {}
        
        start_monitor_time = time.time()
        
        while time.time() - start_monitor_time < monitor_duration:
            for request in submitted_requests:
                if request.request_type == "single_proof":
                    batch_id = request.batch_ids[0]
                    l1_inclusion_block = 4000000  # Default L1 block
                    payload = self.create_single_proof_request(batch_id, l1_inclusion_block)
                else:
                    batch_data = [(batch_id, 4000000) for batch_id in request.batch_ids]
                    payload = self.create_aggregation_request(batch_data)
                
                response = await self.raiko_status_query(payload, request.request_type, request.request_id)
                
                if response.get("status") == "ok" and response.get("data"):
                    task_key = f"{request.request_type}_{request.request_id}"
                    current_status = response["data"].get("status", "unknown").lower()
                    previous_status = task_status_history.get(task_key, "unknown").lower()

                    if current_status != previous_status:
                        self.logger.info(f"STATUS CHANGE: {task_key} - {previous_status} -> {current_status}")
                        task_status_history[task_key] = current_status
                    
                    if task_key not in seen_tasks:
                        if current_status in ["work_in_progress", "registered", "success"]:
                            current_time = time.time()
                            processing_order.append((task_key, request.request_type, payload, current_time, current_status))
                            seen_tasks.add(task_key)
                            
                            if request.request_type == "single_proof":
                                self.logger.info(f"NEW TASK: {request.request_type} for batch {request.batch_ids[0]} - Status: {current_status}")
                            else:
                                self.logger.info(f"NEW TASK: {request.request_type} for batches {request.batch_ids} - Status: {current_status}")
            
            await asyncio.sleep(self.polling_interval)
        
        self.logger.info(f"Monitoring complete. Found {len(processing_order)} new tasks")
        return processing_order

    async def run_priority_test(self, num_single_requests: int = 8, num_aggregation_requests: int = 3, 
                              base_block: int = 4000000, monitor_duration: int = 600) -> bool:
        """Run the main priority test in mixed mode (single and aggregation requests interleaved)"""
        self.logger.info("="*60)
        self.logger.info("STARTING AGGREGATION PRIORITY TEST (MIXED MODE)")
        self.logger.info("="*60)
        self.logger.info(f"Test ID: {self.test_id}")
        self.logger.info(f"Will submit {num_single_requests} single proof requests and {num_aggregation_requests} aggregation requests (mixed)")
        self.logger.info(f"Total requests: {num_single_requests + num_aggregation_requests}")
        
        if not hasattr(self, 'evt_contract') or self.evt_contract is None:
            self.logger.error("L1 contract not initialized - cannot proceed with test")
            return False
        
        self.logger.info(f"Getting batch IDs from L1 chain starting from block {base_block}")
        batch_data = self.get_available_batch_ids(base_block, base_block + 100, num_single_requests + num_aggregation_requests * 3)
        
        if len(batch_data) == 0:
            self.logger.error("No batch proposals found - cannot proceed with test")
            return False
        
        if len(batch_data) < num_single_requests + num_aggregation_requests * 3:
            self.logger.error(f"Not enough batch proposals found. Need {num_single_requests + num_aggregation_requests * 3}, got {len(batch_data)}")
            return False
        
        self.logger.info(f"Found {len(batch_data)} batch proposals to use for testing")
        
        # Create all requests (single and aggregation) and mix them
        mixed_requests = []
        # Single proof requests
        for i in range(num_single_requests):
            batch_id, l1_inclusion_block = batch_data[i]
            payload = self.create_single_proof_request(batch_id, l1_inclusion_block)
            request_id = f"single_{i}_{batch_id}_{self.test_id}"
            mixed_requests.append((payload, request_id, "single_proof"))
        # Aggregation requests
        start_idx = num_single_requests
        for i in range(num_aggregation_requests):
            batch_group = batch_data[start_idx + i * 3:start_idx + (i + 1) * 3]
            payload = self.create_aggregation_request(batch_group)
            batch_ids = [batch_id for batch_id, _ in batch_group]
            request_id = f"aggregation_{i}_{'-'.join(map(str, batch_ids))}_{self.test_id}"
            mixed_requests.append((payload, request_id, "aggregation"))
        # Shuffle the requests to mix single and aggregation
        random.shuffle(mixed_requests)
        self.logger.info(f"Submitting {len(mixed_requests)} requests in mixed order...")
        successful_submissions = 0
        for payload, request_id, request_type in mixed_requests:
            result = await self.submit_request(payload, request_type, request_id)
            if result:
                successful_submissions += 1
            await asyncio.sleep(self.request_delay)
        self.logger.info(f"Submitted {successful_submissions}/{len(mixed_requests)} requests successfully")
        if successful_submissions == 0:
            self.logger.error("No requests were successfully submitted - cannot test priority")
            return False
        # Summary of submitted requests
        successful_requests = [req for req in self.test_requests if req.submission_success]
        single_count = len([r for r in successful_requests if r.request_type == 'single_proof'])
        agg_count = len([r for r in successful_requests if r.request_type == 'aggregation'])
        self.logger.info(f"Summary: {single_count} single proof requests and {agg_count} aggregation requests submitted")

        self.logger.info("Starting to monitor task processing order...")
        self.logger.info("Priority test: Aggregation tasks should start processing before single proof tasks")
        processing_order = await self.monitor_processing(successful_requests, monitor_duration)

        return True


async def main():
    parser = argparse.ArgumentParser(description="Test aggregation request priority")
    
    parser.add_argument(
        "--raiko-rpc",
        type=str,
        default="http://localhost:8080",
        help="Raiko RPC endpoint"
    )
    
    parser.add_argument(
        "--l1-rpc",
        type=str,
        default="https://ethereum-holesky-rpc.publicnode.com",
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
        "--single-requests",
        type=int,
        default=8,
        help="Number of single proof requests to submit"
    )
    
    parser.add_argument(
        "--aggregation-requests", 
        type=int,
        default=3,
        help="Number of aggregation requests to submit"
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
        default=4000000,
        help="Base block number to start testing from"
    )
    
    parser.add_argument(
        "--monitor-duration",
        type=int,
        default=120,
        help="How long to monitor request processing (seconds)"
    )
    
    parser.add_argument(
        "--polling-interval",
        type=int,
        default=120,
        help="Polling interval for status queries (seconds)"
    )
    
    parser.add_argument(
        "--quick-test",
        action="store_true",
        help="Run a quick test with reduced delays and monitoring time"
    )
    
    args = parser.parse_args()
    
    if args.quick_test:
        args.request_delay = 1.0
        args.monitor_duration = 600
        args.polling_interval = 200        
        args.single_requests = 20
        args.aggregation_requests = 5
    
    tester = AggregationPriorityTester(
        raiko_rpc=args.raiko_rpc,
        l1_rpc=args.l1_rpc,
        abi_file=args.abi_file,
        evt_address=args.event_contract,
        log_file=args.log_file,
        prove_type=args.prove_type,
        request_delay=args.request_delay,
        polling_interval=args.polling_interval,
    )
    
    try:
        priority_working = await tester.run_priority_test(
            num_single_requests=args.single_requests,
            num_aggregation_requests=args.aggregation_requests,
            base_block=args.base_block,
            monitor_duration=args.monitor_duration
        )
        
        print("\n" + "="*60)
        if priority_working:
            print("RESULT: Priority queue is working as expected.")
            print("Aggregation requests are being processed with higher priority.")
            exit(0)
        else:
            print("RESULT: Priority queue is not working as expected.")
            print("Single proof requests may be processed before aggregation requests.")
            exit(1)
            
    except KeyboardInterrupt:
        print("\nTest interrupted by user")
        exit(130)
    except Exception as e:
        print(f"Test failed with exception: {e}")
        exit(1)


if __name__ == "__main__":
    asyncio.run(main()) 