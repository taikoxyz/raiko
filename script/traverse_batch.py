#!/usr/bin/env python3

# Usage:
#
# ```
# pip3 install -r script/requirements.txt
#
# python3 ./script/traverse_batch.py --rpc https://l1rpc.internal.taiko.xyz --contract 0xbE71D121291517c85Ab4d3ac65d70F6b1FD57118 --from-block 12700 --to-block 81670
# ```

import argparse
from web3 import Web3
from typing import Dict, Any
import json
from datetime import datetime

# Load ABI from JSON file
with open('script/ITaikoInbox.json', 'r') as f:
    contract_json = json.load(f)
    # Find the BatchProposed event in the ABI
    BATCH_PROPOSED_ABI = next(item for item in contract_json['abi'] if item['type'] == 'event' and item['name'] == 'BatchProposed')

def setup_web3(rpc_url: str) -> Web3:
    """Setup Web3 connection."""
    w3 = Web3(Web3.HTTPProvider(rpc_url))
    if not w3.is_connected():
        raise ConnectionError("Failed to connect to the RPC endpoint")
    return w3

def traverse_batches(w3: Web3, contract_address: str, block_from: int, block_to: int) -> None:
    """Traverse blocks and find BatchProposed events."""
    contract = w3.eth.contract(address=contract_address, abi=[BATCH_PROPOSED_ABI])
    
    # Process blocks in pages of 50
    page_size = 50
    current_from = block_from
    
    while current_from <= block_to:
        current_to = min(current_from + page_size - 1, block_to)
        
        try:
            # Get all events in the current page
            logs = w3.eth.get_logs({
                'address': contract_address,
                # Hint: copy from explorer
                'topics': ["0x9eb7fc80523943f28950bbb71ed6d584effe3e1e02ca4ddc8c86e5ee1558c096"],
                'fromBlock': current_from,
                'toBlock': current_to
            })
            
            for log in logs:
                try:
                    # Decode the event data
                    event = contract.events.BatchProposed().process_log(log)
                    batch_id = event['args']['meta']['batchId']
                    l1_block_number = event['blockNumber']
                    print(f'{batch_id}:{l1_block_number},', end='')
                except Exception as e:
                    print(f"Error processing log: {str(e)}")
                    continue
                    
        except Exception as e:
            print(f"Error getting logs for blocks {current_from}-{current_to}: {str(e)}")
        
        # Move to next page
        current_from = current_to + 1

def main():
    parser = argparse.ArgumentParser(description='Traverse chain and find BatchProposed events')
    parser.add_argument('--rpc', required=True, help='RPC URL of the target chain')
    parser.add_argument('--contract', required=True, help='Contract address of TaikoInbox')
    parser.add_argument('--from-block', type=int, required=True, help='Starting block number')
    parser.add_argument('--to-block', type=int, required=True, help='Ending block number')
    
    args = parser.parse_args()
    
    w3 = setup_web3(args.rpc)
    traverse_batches(w3, args.contract, args.from_block, args.to_block)

if __name__ == "__main__":
    main() 
