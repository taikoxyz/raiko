#!/usr/bin/env bash

# Any error will result in failure
set -e

# # Build gen-verifier first
# cargo build -p sp1-driver --bin gen-verifier --release

network=$1
block=$2

# Function to be called on script exit
cleanup() {
    echo "Stopping the background server"
    kill $SERVER_PID
}

# # Trap exit signals to call cleanup
# trap cleanup EXIT

# # Make sure required artifacts are built
# # $ make install 
# # $ make build
# echo "Running Sp1 prover"
# nohup cargo run > server.log 2>&1 &
# sleep 10

# # Capture the server process ID
# SERVER_PID=$!

# Get the directory of the current script 
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

proofParam='
    "proof_type": "native",
    "native": {
        "json_guest_input": "./provers/sp1/contracts/src/fixtures/input.json"
    }
'
# Function to check the status of prove-block.sh  
check_prove_block_status() {  
    RESPONSE=$("$SCRIPT_DIR/prove-block.sh" ethereum native $block $block "$proofParam")  
    echo $RESPONSE  
    
    DATA_STATUS=$(echo $RESPONSE | jq -r '.data.status')  
    PROOF=$(echo $RESPONSE | jq -r '.data.proof')  
    
    if [ "$DATA_STATUS" == "registered" ]; then  
        echo "Proof in progress..."  
        return 1  
    elif [ "$DATA_STATUS" == "work_in_progress" ]; then  
        echo "Proof in progress..."  
        return 1  
    elif [ "$DATA_STATUS" == "unspecified-failure-reason" ]; then  
        echo "Proof Failed"  
        exit 1
    elif [ "$PROOF" != "" ]; then  
        echo "Proof completed:"  
        echo $PROOF  
        return 0  
    else  
        echo "Unexpected status: $DATA_STATUS"  
        return 1  
    fi  
}  

# Loop until prove-block.sh is successful  
while ! check_prove_block_status; do  
    sleep 5  # wait for 5 seconds before the next check  
done 

# Generate solidity tests fixture
cargo run -p sp1-driver --bin gen-verifier --release --features enable

# Run Smart Contract verification
# cd $SCRIPT_DIR/../provers/sp1/contracts
# forge test -v

# Manually call cleanup at the end to stop the server
cleanup