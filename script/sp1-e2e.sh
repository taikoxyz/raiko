#!/usr/bin/env bash

# Any error will result in failure
set -e

# Build gen-verifier first
cargo build -p sp1-driver --bin gen-verifier --release

block=$1

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
sleep 5

# Capture the server process ID
SERVER_PID=$!

# Get the directory of the current script 
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

proofParam='
    "proof_type": "native",
    "native": {
        "save_test_input": true
    }
'
# Function to check if prove-block.sh is successful
check_prove_block() {
    "$SCRIPT_DIR/prove-block.sh" taiko_a7 native $block $block "$proofParam"
    return $?
}

# Loop until prove-block.sh is successful
while ! check_prove_block; do
    echo "Waiting for server to be ready..."
    sleep 5
done

# Generate solidity tests fixture
RUST_LOG=info cargo run -p sp1-driver --bin gen-verifier --release --features enable

# Run Smart Contract verification
cd $SCRIPT_DIR/../provers/sp1/contracts
forge test

# Manually call cleanup at the end to stop the server
cleanup