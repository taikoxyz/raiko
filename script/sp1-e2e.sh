#!/usr/bin/env bash

# Any error will result in failure
set -e

TOOLCHAIN_SP1=+nightly-2024-02-06
block=$1

# Make sure required artifacts are built
# $ make install 
# $ make build
echo "Running Sp1 prover"
cargo ${TOOLCHAIN_SP1} run --features sp1

# Wait for the service to be ready
sleep 5

# Save GuestInput from block 3456 
proofParam='
    "proof_type": "sp1",
    "sp1": {
        "recursion": "core",
        "prover": "mock",
        "save_test_input": true
    }
'
# Get the directory of the current script 
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$SCRIPT_DIR/prove-block.sh" taiko_a7 sp1 $block $block "$proofParam"

# Generate solidity tests fixture
cargo run -p sp1-driver --bin gen-verifier

# Run Smart Contract verification
cd $SCRIPT_DIR/../provers/sp1/contracts
forge test 