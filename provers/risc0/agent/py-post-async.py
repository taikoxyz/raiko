#!/usr/bin/env python3
"""
Test script for boundless agent endpoints (all requests are async by default).
Usage: python py-post-async.py tests/fixtures/batch-input-1490225.bin
"""
import json
import requests
import sys
import time

# Configuration
PORT = 9999  # Change this to test against different ports

def test_boundless_endpoints(input_file):
    # Read binary input
    with open(input_file, 'rb') as f:
        binary_data = f.read()
    input_bytes = list(binary_data)
    
    print(f"Testing boundless agent with input file: {input_file}")
    print(f"Input size: {len(binary_data)} bytes")
    
    # 1. Submit proof (async by default)
    print("\n1. Submitting proof request...")
    response = requests.post(
        f"http://localhost:{PORT}/proof",
        json={
            "input": input_bytes,
            "proof_type": "Batch",
            "config": {}
        }
    )
    
    if response.status_code != 202:
        print(f"ERROR: Failed to submit async proof: {response.text}")
        return
        
    result = response.json()
    async_request_id = result["request_id"]
    print(f"✓ Proof submitted successfully!")
    print(f"  Request ID: {async_request_id}")
    print(f"  Status: {result['status']}")
    print(f"  Message: {result['message']}")
    
    # 2. Check status periodically
    print(f"\n2. Monitoring status for request: {async_request_id}")
    max_attempts = 20  # Check for ~2 minutes
    
    for attempt in range(max_attempts):
        response = requests.get(f"http://localhost:{PORT}/status/{async_request_id}")
        
        if response.status_code == 200:
            status_data = response.json()
            print(f"  [{attempt+1}/{max_attempts}] Status: {status_data['status']}")
            print(f"    Message: {status_data.get('status_message', 'No message')}")
            
            # Check if completed
            status_str = str(status_data['status'])
            if "Fulfilled" in status_str:
                print("Proof completed successfully!")
                break
            elif "Timeout" in status_str or "Failed" in status_str:
                print("Proof failed or timed out!")
                break
        else:
            print(f"  [{attempt+1}/{max_attempts}] ERROR: {response.status_code} - {response.text}")
        
        if attempt < max_attempts - 1:  # Don't sleep on last attempt
            time.sleep(30)  # Wait 6 seconds between checks
    
    # 3. List all active requests
    print(f"\n3. Listing all active requests...")
    response = requests.get(f"http://localhost:{PORT}/requests")
    
    if response.status_code == 200:
        data = response.json()
        print(f"Active requests: {data['active_requests']}")
        if data['requests']:
            for req in data['requests']:
                print(f"  - {req['request_id']}: {req['status']}")
    else:
        print(f"ERROR: Failed to list requests: {response.text}")

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <input_file>")
        print(f"Example: {sys.argv[0]} tests/fixtures/batch-input-1490225.bin")
        sys.exit(1)
    
    test_boundless_endpoints(sys.argv[1])