#!/usr/bin/env bash

# Any error will result in failure
set -e

network=$1  
block=$2  

# Check if both network and block are set  
if [ -z "$network" ] || [ -z "$block" ]; then  
    echo "Error: Both 'network' and 'block' arguments must be provided."  
    echo "Usage: $0 <network> <block>"  
    exit 1  
fi  

# Construct the input file name  
input_filename="input-${network}-${block}.json"  

# Define the path where the input file is located  
input_path="./provers/sp1/contracts/src/fixtures"  

# Create the proofParam JSON string using a here-document  
proofParam=$(cat <<EOF  
    "proof_type": "native",  
    "native": {  
        "json_guest_input": "${input_path}/${input_filename}"  
    }  
EOF
)

# Output the proofParam JSON  
echo "$proofParam"

# Function to be called on script exit
cleanup() {
    echo "Stopping the background server"
    kill $SERVER_PID
}

# Trap exit signals to call cleanup
trap cleanup EXIT

# Make sure required artifacts are built
# $ make install 
# $ make build
echo "Running Sp1 prover"
nohup cargo run > server.log 2>&1 &
sleep 10

# Capture the server process ID
SERVER_PID=$!

# Get the directory of the current script 
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Function to check the status of prove-block.sh  
check_prove_block_status() {  
    RESPONSE=$("$SCRIPT_DIR/prove-block.sh" $network native $block $block "$proofParam")  
    echo "$RESPONSE"  # Debugging line  

    # Extract the JSON part of the response  
    JSON_RESPONSE=$(echo "$RESPONSE" | tail -n 1) 

    # Sanity check for JSON validity  
    if ! echo "$JSON_RESPONSE" | jq . >/dev/null 2>&1; then  
        echo "ERROR: Received invalid JSON:"  
        echo "$JSON_RESPONSE"  
        return 1  
    fi  

    # Determine if the response contains a status or proof  
    if echo "$JSON_RESPONSE" | jq -e '.data.status' >/dev/null 2>&1; then  
        DATA_STATUS=$(echo "$JSON_RESPONSE" | jq -r '.data.status')  
        echo "not done ..."
        if [ "$DATA_STATUS" == "unspecified_failure_reason" ]; then  
            echo "Proof Failed"  
            exit 1 
        fi
        return 1
    elif  echo "$JSON_RESPONSE" | jq -e '.data.proof' >/dev/null 2>&1; then  
        PROOF=$(echo "$JSON_RESPONSE" | jq -r '.data.proof')  
        echo "done!"
        return 0  
    else  
        echo "Unexpected status: $DATA_STATUS"  
        return 0  
    fi  
}  

while ! check_prove_block_status; do  
    sleep 5  
done

# Generate solidity tests fixture
cargo run --bin run-verifier --release -- $input_filename

# Run Smart Contract verification
cd $SCRIPT_DIR/../provers/sp1/contracts
forge test -v

# Manually call cleanup at the end to stop the server
cleanup