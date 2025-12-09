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


@dataclass
class RaikoResponse:
    status: str
    data: Optional[Dict[str, Any]] = None
    message: Optional[str] = None


class BatchMonitor:
    def __init__(
        self,
        l1_rpc: str,
        abi_file: str,
        evt_address: str,
        raiko_rpc: str,
        log_file: str = "block_monitor.log",
        polling_interval: int = 3,
        max_retries: int = 3,
        block_running_ratio: float = 0.1,
        block_range: Optional[Tuple[int, int]] = None,
        timeout: int = 3600,  # 1 hour
        prove_type: str = "native",
        watch_mode: bool = False,
        time_speed: float = 1.0,
    ):
        self.l1_rpc = l1_rpc
        self.raiko_rpc = raiko_rpc
        self.log_file = log_file
        self.block_polling_interval = polling_interval
        self.task_polling_interval = polling_interval
        self.max_retries = max_retries
        self.timeout = timeout
        self.last_block = None
        self.batchs_in_last_block = deque()
        self.block_running_ratio = block_running_ratio
        self.block_range = block_range
        self.ts_offset: Optional[int] = None
        self.last_block_ts_in_real_world: int = 0
        self.running_count = 0
        self.prove_type = prove_type
        self.watch_mode = watch_mode
        self.time_speed = time_speed
        # logger
        logging.basicConfig(
            level=logging.INFO,
            format="%(asctime)s - %(levelname)s - %(message)s",
            handlers=[logging.FileHandler(log_file), logging.StreamHandler()],
        )
        self.logger = logging.getLogger(__name__)
        self.__init_contract_event(l1_rpc, abi_file, evt_address)

    def __init_contract_event(self, l1_rpc, abi_file, evt_address):
        print(f"l1_rpc = {l1_rpc}, abi_file = {abi_file}, evt_address = {evt_address}")
        with open(abi_file) as f:
            abi = json.load(f)
        l1_w3 = Web3(Web3.HTTPProvider(l1_rpc, {"timeout": 10}))
        l1_w3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)
        if l1_w3.is_connected():
            self.logger.info(f"Connected to l1 node {l1_rpc}")
        self.evt_contract = l1_w3.eth.contract(address=evt_address, abi=abi["abi"])

    def parse_batch_proposed_meta(self, log):
        try:
            parsed_log = self.evt_contract.events.BatchProposed().process_log(log)
            meta = parsed_log.args.meta
            return meta.batchId
        except Exception as e:
            return None

    def get_batch_events_in_block(self, block_number) -> list[int]:
        try:
            logs = self.evt_contract.events.BatchProposed().get_logs(
                from_block=block_number, to_block=block_number
            )
            return [log.args.meta.batchId for log in logs]
        except Exception as e:
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
        logs = self.evt_contract.events.BatchProposed().get_logs(
            from_block="latest", to_block="latest"
        )
        if len(logs) == 0:
            return None
        return logs[0].blockNumber, [log.args.meta.batchId for log in logs]

    def generate_post_data(
        self, batch_id: int, batch_proposal_height: int
    ) -> Dict[str, Any]:
        """generate post data"""
        return {
            "batches": [
                {
                    "batch_id": batch_id,
                    "l1_inclusion_block_number": batch_proposal_height,
                }
            ],
            "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
            "graffiti": "8008500000000000000000000000000000000000000000000000000000000000",
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
            "sp1": {"recursion": "plonk", "prover": "network", "verify": True},
        }

    async def submit_to_raiko(
        self, batch_id: int, batch_inclusion_block: int
    ) -> Optional[str]:
        """submit batch to Raiko"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}

            payload = self.generate_post_data(batch_id, batch_inclusion_block)
            print(f"payload = {payload}")

            response = requests.post(
                f"{self.raiko_rpc}/v3/proof/batch",
                headers=headers,
                json=payload,
                timeout=10,
            )
            result = response.json()
            if "data" in result:
                result["data"] = {}  # avoid big print
            if result.get("status") == "ok":
                self.logger.info(
                    f"Batch {batch_id} in block {batch_inclusion_block} submitted to Raiko with response: {result}"
                )
                return None
            else:
                self.logger.error(
                    f"Failed to submit block: {result.get('message', 'Unknown error')}"
                )
                return None
        except Exception as e:
            self.logger.error(f"Failed to submit to Raiko: {e}")
            return None

    async def query_raiko_status(
        self, batch_id: int, batch_inclusion_block: int
    ) -> RaikoResponse:
        """query Raiko status"""
        try:
            headers = {"x-api-key": "1", "Content-Type": "application/json"}
            payload = self.generate_post_data(batch_id, batch_inclusion_block)
            response = requests.post(
                f"{self.raiko_rpc}/v3/proof/batch",
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

    async def process_batch(self, batch_id: int, l1_inclusion_block: int):
        """handle new batch"""
        try:
            if self.watch_mode:
                self.logger.info(f"Watch mode, skip processing")
                return

            start_time = datetime.now()
            self.logger.info(
                f"Starting to process batch {batch_id} @ {l1_inclusion_block} at {start_time}"
            )

            with open(self.log_file, "a") as f:
                f.write(
                    f"\nbatch {batch_id} in block {l1_inclusion_block} processing started at {start_time}\n"
                )

            # request Raiko
            await self.submit_to_raiko(batch_id, l1_inclusion_block)

            # polling
            retry_count = 0
            start_polling_time = time.time()

            while True:
                if time.time() - start_polling_time > self.timeout:
                    self.logger.error(
                        f"Timeout waiting for batch {batch_id} / {l1_inclusion_block}"
                    )
                    break

                response = await self.query_raiko_status(batch_id, l1_inclusion_block)

                if response.status == "error":
                    self.logger.error(
                        f"Error processing batch {batch_id} in block {l1_inclusion_block}: {response.message}"
                    )
                    retry_count += 1
                    if retry_count >= self.max_retries:
                        self.logger.error(
                            f"Max retries reached for batch {batch_id} in block {l1_inclusion_block}"
                        )
                        break

                if response.data:
                    retry_count = 0  # reset retry count
                    if response.data.get("status") == "registered":
                        self.logger.info(
                            f"Batch {batch_id} in Block {l1_inclusion_block} registered"
                        )
                    elif response.data.get("status") == "work_in_progress":
                        self.logger.info(
                            f"Batch {batch_id} in Block {l1_inclusion_block} in progress"
                        )
                    elif response.data.get("proof"):
                        self.logger.info(
                            f"Batch {batch_id} in Block {l1_inclusion_block} completed with proof {response.data['proof']['proof']}"
                        )
                        break
                    else:
                        self.logger.warning(
                            f"Batch {batch_id} in Block {l1_inclusion_block} unhandled status: {response}"
                        )

                await asyncio.sleep(self.task_polling_interval)

            end_time = datetime.now()
            duration = (end_time - start_time).total_seconds()

            # log ending status
            with open(self.log_file, "a") as f:
                f.write(
                    f"Block {l1_inclusion_block} processed {response.status} at {end_time}, duration: {duration} seconds\n"
                )
                if response.message:
                    f.write(f"Message: {response.message}\n")
                if response.data and response.data.get("proof"):
                    f.write(f"Proof: {response.data['proof']['proof']}\n")
        finally:
            self.running_count -= 1
            self.logger.info(
                f"Block {l1_inclusion_block} processed, remaining running: {self.running_count}"
            )

    async def run(self):
        """main loop"""
        self.logger.info("Starting block monitor")
        # print start config
        self.logger.info(f"Config:\n{json.dumps(self.__dict__, indent=2, default=str)}")
        acc_odds = 1
        while True:
            try:
                result = await self.get_next_batches()
                if result is not None:
                    current_block, batch_ids = result
                    for batch_id in batch_ids:
                        self.logger.info(
                            f"New batch detected: {batch_id}@{current_block}"
                        )
                        acc_odds += self.block_running_ratio
                        if acc_odds >= 1.0:
                            self.logger.info(
                                f"To run batch/block: {batch_id}/{current_block}, current running tasks: {self.running_count}"
                            )
                            self.running_count += 1
                            acc_odds -= 1.0
                            asyncio.create_task(
                                self.process_batch(batch_id, current_block)
                            )
                        else:
                            self.logger.info(
                                f"Block {current_block} skipped due to block_running_ratio:{self.block_running_ratio}"
                            )
                        self.last_block = current_block
                await asyncio.sleep(self.block_polling_interval)
            except Exception as e:
                self.logger.error(f"Error in main loop: {e}")
                await asyncio.sleep(self.block_polling_interval)


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
        help='Ethereum RPC endpoint (use "none" for None value)',
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
        "--block-range",
        type=parse_block_range,
        default="None",
        help='Block range in format "start,end" or "none" for None value',
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
        default="./ITaikoInbox.json",
        help='Prove type (use "none" for None value)',
    )

    parser.add_argument(
        "-c",
        "--event-contract",
        type=lambda x: parse_none_value(x, str),
        default="ITaikoInbox.json",
        help='Prove type (use "none" for None value)',
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

    args = parser.parse_args()

    monitor = BatchMonitor(
        l1_rpc=args.l1_rpc,
        abi_file=args.abi_file,
        evt_address=web3.Web3.to_checksum_address(args.event_contract),
        raiko_rpc=args.raiko_rpc,
        log_file=args.log_file,
        polling_interval=args.polling_interval,
        block_running_ratio=args.block_running_ratio,
        block_range=args.block_range,
        prove_type=args.prove_type,
        watch_mode=args.watch_event,
        time_speed=args.time_speed,
    )

    await monitor.run()

# python stress_batch.py -t native -g 8950,8960 -p 3 -o stress_dev.log -c '0xbE71D121291517c85Ab4d3ac65d70F6b1FD57118' #devnet
# python stress_batch.py -t native -g 1780200,1780240 -p 3 -o stress_dev.log -c 0xf6eA848c7d7aC83de84db45Ae28EAbf377fe0eF9 -e https://ethereum-hoodi-rpc.publicnode.com #hoodi
if __name__ == "__main__":
    asyncio.run(main())
