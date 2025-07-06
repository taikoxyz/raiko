#!/usr/bin/env python3
"""
Test script to verify that aggregation requests get the highest priority.

Usage Examples:
  # Basic test with default settings
  python3 test_aggregation_priority.py
  
  # Quick test with shorter delays and monitoring
  python3 test_aggregation_priority.py --quick-test
  
  # Custom test with specific parameters
  python3 test_aggregation_priority.py --single-requests 3 --aggregation-requests 2 --base-block 10000
  
  # Test with different prover and longer monitoring
  python3 test_aggregation_priority.py --prove-type sgx --monitor-duration 300

"""

import asyncio
import json
import time
import logging
from typing import List, Dict, Any, Optional, Set, Tuple
from datetime import datetime
import argparse
import requests
from dataclasses import dataclass
import hashlib
import random


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
    block_numbers: List[int]
    start_processing_time: Optional[float] = None
    completion_time: Optional[float] = None
    submission_success: bool = False


class AggregationPriorityTester:
    def __init__(
        self,
        raiko_rpc: str = "http://localhost:8080",
        log_file: str = "aggregation_priority_test.log",
        timeout: int = 3600,
        prove_type: str = "native",
        request_delay: float = 5.0,
        polling_interval: int = 30,
        max_retries: int = 3,
    ):
        self.raiko_rpc = raiko_rpc
        self.log_file = log_file
        self.timeout = timeout
        self.prove_type = prove_type
        self.request_delay = request_delay
        self.polling_interval = polling_interval
        self.max_retries = max_retries
        self.test_requests: List[TestRequest] = []
        
        # Track which blocks belong to which request type for priority analysis
        self.single_request_blocks: Set[int] = set()
        self.aggregation_request_blocks: Set[int] = set()
        
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

    def create_single_proof_request(self, block_number: int) -> Dict[str, Any]:
        """Create a single proof request payload for /v3/proof endpoint"""
        return {
            "block_numbers": [(block_number, None)],
            "network": "holesky",
            "l1_network": "holesky", 
            "graffiti": "8008500000000000000000000000000000000000000000000000000000000000",
            "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
            "blob_proof_type": "proof_of_equivalence",
            "proof_type": self.prove_type,
            "native": {},
            "sgx": {
                "instance_id": 1234,
                "setup": False,
                "bootstrap": False,
                "prove": True,
            },
            "risc0": {
                "bonsai": True,
                "snark": True,
                "profile": False,
                "execution_po2": 20,
            },
            "sp1": {
                "recursion": "plonk",
                "prover": "network",
                "verify": True
            },
        }

    def create_aggregation_request(self, block_numbers: List[int]) -> Dict[str, Any]:
        """Create an aggregation request payload for /v3/proof endpoint (multiple blocks)"""
        block_tuples = [(block_num, None) for block_num in block_numbers]
        
        return {
            "block_numbers": block_tuples,
            "network": "holesky",
            "l1_network": "holesky",
            "graffiti": "8008500000000000000000000000000000000000000000000000000000000000",
            "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
            "proof_type": self.prove_type,
            "blob_proof_type": "proof_of_equivalence",
            "native": {},
            "sgx": {
                "instance_id": 1234,
                "setup": False,
                "bootstrap": False,
                "prove": True,
            },
            "risc0": {
                "bonsai": True,
                "snark": True,
                "profile": False,
                "execution_po2": 20,
            },
            "sp1": {
                "recursion": "plonk",
                "prover": "network",
                "verify": True
            },
        }

    async def submit_request(self, payload: Dict[str, Any], request_type: str, request_id: str) -> Optional[Dict[str, Any]]:
        """Submit a request to Raiko and return the request parameters if successful"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            endpoint = f"{self.raiko_rpc}/v3/proof"
            
            self.logger.info(f"Submitting {request_type} request {request_id} to {endpoint}")
            
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

            # Extract block numbers from the payload
            if request_type == "single_proof":
                block_numbers = [block[0] for block in payload["block_numbers"]]
            else:  # aggregation
                block_numbers = [block[0] for block in payload["block_numbers"]]
            
            # Create test request record
            test_request = TestRequest(
                request_type=request_type,
                request_id=request_id,
                submitted_time=start_time,
                block_numbers=block_numbers,
                submission_success=response.status_code == 200
            )
            self.test_requests.append(test_request)
            
            if request_type == "single_proof":
                self.single_request_blocks.update(block_numbers)
            elif request_type == "aggregation":
                self.aggregation_request_blocks.update(block_numbers)
            
            if response.status_code == 200:
                self.logger.info(f"Successfully submitted {request_type} request {request_id}")
                self.logger.info(f"Response body for {request_type} request {request_id}: {result}")
                return {
                    "request_type": request_type,
                    "request_id": request_id,
                    "payload": payload,
                    "block_numbers": block_numbers,
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

    async def raiko_status_query(self, payload: Dict[str, Any], request_type: str, request_id: str) -> RaikoResponse:
        """Query Raiko status for a submitted request"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            endpoint = f"{self.raiko_rpc}/v3/proof"
            
            response = requests.post(
                endpoint,
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
            self.logger.error(f"Failed to query Raiko status for {request_type} request {request_id}: {e}")
            return RaikoResponse(status="error", message=str(e))

    async def monitor_request_processing(self, submitted_requests: List[TestRequest], monitor_duration: int = 300):
        """Monitor the processing order of submitted requests using status queries"""
        self.logger.info(f"Monitoring request processing for {monitor_duration} seconds...")
        self.logger.info(f"Single proof blocks: {sorted(self.single_request_blocks)}")
        self.logger.info(f"Aggregation blocks: {sorted(self.aggregation_request_blocks)}")
        
        processing_order = []
        seen_tasks = set()
        task_start_times = {}
        task_status_history = {}
        
        start_monitor_time = time.time()
        
        baseline_tasks = set()
        for request in submitted_requests:
            if request.request_type == "single_proof":
                payload = self.create_single_proof_request(request.block_numbers[0])
            else:
                payload = self.create_aggregation_request(request.block_numbers)
            
            response = await self.raiko_status_query(payload, request.request_type, request.request_id)
            if response.status == "ok" and response.data:
                task_key = f"{request.request_type}_{request.request_id}"
                baseline_tasks.add(task_key)
                current_status = response.data.get('status', 'unknown')
                task_status_history[task_key] = current_status
                self.logger.debug(f"Baseline task: {task_key} - Status: {current_status}")
        
        self.logger.info(f"Baseline captured {len(baseline_tasks)} existing tasks")
        
        
        while time.time() - start_monitor_time < monitor_duration:
            for request in submitted_requests:
                if request.request_type == "single_proof":
                    payload = self.create_single_proof_request(request.block_numbers[0])
                else:
                    payload = self.create_aggregation_request(request.block_numbers)
                
                response = await self.raiko_status_query(payload, request.request_type, request.request_id)
                
                if response.status == "ok" and response.data:
                    task_key = f"{request.request_type}_{request.request_id}"
                    current_status = response.data.get("status", "unknown")
                    previous_status = task_status_history.get(task_key, "unknown")
                    
                    if current_status in ["success", "error", "failed"]:
                        continue
                    
                    # Only log if status changed
                    if current_status != previous_status:
                        self.logger.info(f"STATUS CHANGE: {task_key} - {previous_status} -> {current_status}")
                        task_status_history[task_key] = current_status
                    
                    # Only process new tasks not in baseline
                    if task_key not in seen_tasks:
                        # Only consider when a task first enters 'work_in_progress'
                        if current_status == "work_in_progress":
                            current_time = time.time()
                            task_start_times[task_key] = current_time
                            processing_order.append((task_key, request.request_type, payload, current_time, current_status))
                            seen_tasks.add(task_key)
                            # Log the task details
                            if request.request_type == "single_proof":
                                block_id = None
                                if "block_numbers" in payload and payload["block_numbers"]:
                                    block_id = payload["block_numbers"][0][0]
                                self.logger.info(f"NEW TASK: {request.request_type} SingleProof for block {block_id} - Status: {current_status}")
                            elif request.request_type == "aggregation":
                                block_numbers = [block[0] for block in payload.get("block_numbers", [])]
                                self.logger.info(f"NEW TASK: {request.request_type} Aggregation for blocks {block_numbers} - Status: {current_status}")
            await asyncio.sleep(self.polling_interval)
        self.logger.info(f"Monitoring complete. Found {len(processing_order)} new tasks (work_in_progress only)")
        return processing_order

    def analyze_priority_results(self, processing_order: List[Tuple]) -> bool:
        """Analyze the processing order to verify priority queue behavior (work_in_progress only)"""
        self.logger.info("\n" + "="*60)
        self.logger.info("PRIORITY QUEUE ANALYSIS (work_in_progress only)")
        self.logger.info("="*60)
        
        if not processing_order:
            self.logger.warning("No new tasks entered 'work_in_progress' during monitoring period")
            self.logger.warning("This could indicate:")
            self.logger.warning("  - All tasks completed before monitoring started")
            self.logger.warning("  - Tasks are queued but not yet started")
            self.logger.warning("  - Request submission failed")
            return False
        
        self.logger.info(f"Processing order ({len(processing_order)} tasks):")
        
        single_proof_start_times = []
        aggregation_start_times = []
        
        for i, (task_key, task_type, payload, start_time, status) in enumerate(processing_order, 1):
            timestamp = datetime.fromtimestamp(start_time).strftime('%H:%M:%S.%f')[:-3]
            
            if task_type == "single_proof":
                block_id = None
                if "block_numbers" in payload and payload["block_numbers"]:
                    block_id = payload["block_numbers"][0][0]
                self.logger.info(f"  {i}. {task_type} SingleProof (block {block_id}) - {timestamp} - {status}")
            elif task_type == "aggregation":
                block_numbers = [block[0] for block in payload.get("block_numbers", [])]
                self.logger.info(f"  {i}. {task_type} Aggregation (blocks {block_numbers}) - {timestamp} - {status}")
            else:
                self.logger.info(f"  {i}. {task_type} task - {timestamp} - {status}")
            
            if task_type == "single_proof":
                single_proof_start_times.append(start_time)
            elif task_type == "aggregation":
                aggregation_start_times.append(start_time)
        
        # Detailed priority analysis
        priority_working = self._analyze_priority_logic(single_proof_start_times, aggregation_start_times)
        
        # Summary statistics
        self._print_priority_statistics(processing_order, single_proof_start_times, aggregation_start_times)
        
        self.logger.info("="*60)
        return priority_working

    def _analyze_priority_logic(self, single_proof_times: List[float], aggregation_times: List[float]) -> bool:
        """Core priority verification logic"""
        if not aggregation_times and not single_proof_times:
            self.logger.error("No tasks processed - cannot verify priority")
            return False
        
        if not aggregation_times:
            self.logger.warning("No aggregation-related tasks processed")
            self.logger.warning("This suggests aggregation requests may not be working correctly")
            return False
        
        if not single_proof_times:
            self.logger.info("Only aggregation-related tasks processed")
            self.logger.info("Priority cannot be compared without single proof tasks")
            return True
        
        earliest_aggregation = min(aggregation_times)
        earliest_single = min(single_proof_times)
        
        self.logger.info(f"\nPriority Analysis:")
        self.logger.info(f"  Earliest aggregation task: {datetime.fromtimestamp(earliest_aggregation).strftime('%H:%M:%S.%f')[:-3]}")
        self.logger.info(f"  Earliest single proof task: {datetime.fromtimestamp(earliest_single).strftime('%H:%M:%S.%f')[:-3]}")
        
        time_diff = earliest_single - earliest_aggregation
        
        if earliest_aggregation <= earliest_single:
            self.logger.info(f"PRIORITY WORKING: Aggregation tasks started {abs(time_diff):.3f}s before single proof tasks")
            return True
        else:
            self.logger.error(f"PRIORITY NOT WORKING: Single proof tasks started {time_diff:.3f}s before aggregation tasks")
            return False

    def _print_priority_statistics(self, processing_order: List[Tuple], single_times: List[float], agg_times: List[float]):
        """Print detailed statistics about priority behavior"""
        if not single_times or not agg_times:
            return
        
        agg_before_single = sum(1 for agg_time in agg_times 
                              for single_time in single_times 
                              if agg_time < single_time)
        total_comparisons = len(agg_times) * len(single_times)
        
        if total_comparisons > 0:
            priority_ratio = agg_before_single / total_comparisons
            self.logger.info(f"Priority ratio: {agg_before_single}/{total_comparisons} = {priority_ratio:.2%}")
        
        all_times = single_times + agg_times
        time_span = max(all_times) - min(all_times)
        
        self.logger.info(f"Task timing spread: {time_span:.3f} seconds")
        self.logger.info(f"Aggregation tasks: {len(agg_times)}")
        self.logger.info(f"Single proof tasks: {len(single_times)}")

    async def run_priority_test(self, num_single_requests: int = 2, num_aggregation_requests: int = 1, 
                              base_block: int = 14000, monitor_duration: int = 300) -> bool:
        """Run the main priority test"""
        self.logger.info("="*60)
        self.logger.info("STARTING AGGREGATION PRIORITY TEST")
        self.logger.info("="*60)
        self.logger.info(f"Test ID: {self.test_id}")
        self.logger.info(f"Will submit {num_single_requests} single proof requests and {num_aggregation_requests} aggregation requests")
        
        single_requests = []
        aggregation_requests = []
        
        for i in range(num_single_requests):
            block_number = base_block + i * 1000
            payload = self.create_single_proof_request(block_number)
            request_id = f"single_{i}_{block_number}_{self.test_id}"
            single_requests.append((payload, request_id))
        
        # Create aggregation requests with different block ranges
        for i in range(num_aggregation_requests):
            block_numbers = [base_block + 2000 + i * 5000 + j * 100 for j in range(3)]
            payload = self.create_aggregation_request(block_numbers)
            request_id = f"aggregation_{i}_{'-'.join(map(str, block_numbers))}_{self.test_id}"
            aggregation_requests.append((payload, request_id))
        
        self.logger.info(f"Using base block number: {base_block}")
        self.logger.info(f"Single proof blocks: {[base_block + i * 1000 for i in range(num_single_requests)]}")
        self.logger.info(f"Aggregation blocks: {[[base_block + 10000 + i * 5000 + j * 100 for j in range(3)] for i in range(num_aggregation_requests)]}")

        # Submit all requests with staggered timing
        all_requests = []
        
        # Interleave single and aggregation requests
        max_requests = max(len(single_requests), len(aggregation_requests))
        for i in range(max_requests):
            if i < len(single_requests):
                all_requests.append(("single_proof", single_requests[i]))
            if i < len(aggregation_requests):
                all_requests.append(("aggregation", aggregation_requests[i]))
        
        self.logger.info("Submitting all requests...")
        successful_submissions = 0

        for request_type, (payload, request_id) in all_requests:
            # Submit request
            result = await self.submit_request(payload, request_type, request_id)
            if result:
                successful_submissions += 1
            
            await asyncio.sleep(self.request_delay)
        
        self.logger.info(f"Submitted {successful_submissions}/{len(all_requests)} requests successfully")
        
        if successful_submissions == 0:
            self.logger.error("No requests were successfully submitted - cannot test priority")
            return False
        
        # Summary of submitted requests
        successful_requests = [req for req in self.test_requests if req.submission_success]
        single_count = len([r for r in successful_requests if r.request_type == 'single_proof'])
        agg_count = len([r for r in successful_requests if r.request_type == 'aggregation'])
        self.logger.info(f"Summary: {single_count} single proof requests and {agg_count} aggregation requests submitted")
        
        # Wait for requests to be processed and queued
        queue_wait_time = 60
        self.logger.info(f"Waiting {queue_wait_time} seconds for requests to be queued...")
        await asyncio.sleep(queue_wait_time)
        
        # Monitor processing order
        processing_order = await self.monitor_request_processing(successful_requests, monitor_duration)
        
        # Analyze results
        priority_working = self.analyze_priority_results(processing_order)
        
        return priority_working


async def main():
    parser = argparse.ArgumentParser(description="Test aggregation request priority")
    
    parser.add_argument(
        "--raiko-rpc",
        type=str,
        default="http://localhost:8080",
        help="Raiko RPC endpoint"
    )
    
    parser.add_argument(
        "--log-file",
        type=str,
        default="aggregation_priority_test.log",
        help="Log file path"
    )
    
    parser.add_argument(
        "--timeout",
        type=int,
        default=3600,
        help="Timeout in seconds"
    )
    
    parser.add_argument(
        "--single-requests",
        type=int,
        default=2,
        help="Number of single proof requests to submit"
    )
    
    parser.add_argument(
        "--aggregation-requests", 
        type=int,
        default=1,
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
        default=5.0,
        help="Delay between submissions to avoid RPC rate limiting"
    )
    
    parser.add_argument(
        "--base-block",
        type=int,
        default=14000,
        help="Base block number to start testing from (should be a confirmed block)"
    )
    
    parser.add_argument(
        "--monitor-duration",
        type=int,
        default=300,
        help="How long to monitor request processing (seconds)"
    )
    
    parser.add_argument(
        "--polling-interval",
        type=int,
        default=2,
        help="Polling interval for status queries (seconds)"
    )
    
    parser.add_argument(
        "--quick-test",
        action="store_true",
        help="Run a quick test with reduced delays and monitoring time"
    )
    
    args = parser.parse_args()
    
    if args.quick_test:
        args.request_delay = 2.0
        args.monitor_duration = 120
        args.polling_interval = 15
        print("Quick test mode: reduced delays and monitoring time")
    
    tester = AggregationPriorityTester(
        raiko_rpc=args.raiko_rpc,
        log_file=args.log_file,
        timeout=args.timeout,
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