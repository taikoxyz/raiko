#!/usr/bin/env bash

# Environment variables
# risc0
BONSAI_API_KEY=$BONSAI_API_KEY
BONSAI_API_URL=$BONSAI_API_URL
# reference verifier was deployed in holesky
GROTH16_VERIFIER_RPC_URL=https://ethereum-holesky-rpc.publicnode.com
# v1.1.2
GROTH16_VERIFIER_ADDRESS=0x3098D568e8E7E158D1a84233182C5fC60f50c16a

# sp1
SP1_PROVER=network
SKIP_SIMULATION=true
PROVER_NETWORK_RPC=$PROVER_NETWORK_RPC
SP1_PRIVATE_KEY=$SP1_PRIVATE_KEY
# reference verifier was deployed in holesky
SP1_VERIFIER_RPC_URL=https://ethereum-holesky-rpc.publicnode.com
# v2.0.0
# export SP1_VERIFIER_ADDRESS=0x35500C6fdfc4d57582672CE32A55B9a3fB48292d
# v3.0.0-rc3
SP1_VERIFIER_ADDRESS=0x7089666D4fEcca134D1a6d2636b03e2087B52e97

# Function to set environment variable persistently
set_persistent_env() {
    local var_name="$1"
    local var_value="$2"
    local file="$HOME/.bashrc"

    # Check if the variable assignment already exists in the file
    if ! grep -q "export $var_name=" "$file"; then
        # Append export to .bashrc if not already present
        echo "export $var_name=\"$var_value\"" >> "$file"
        echo "$var_name=$var_value"
    else
        # Update the existing entry
        sed -i "/export $var_name=/c\export $var_name=\"$var_value\"" "$file"
        echo "$var_name=$var_value"
    fi
}

# Set each variable persistently
set_persistent_env "BONSAI_API_KEY" "$BONSAI_API_KEY"
set_persistent_env "BONSAI_API_URL" "$BONSAI_API_URL"
set_persistent_env "GROTH16_VERIFIER_ADDRESS" "$GROTH16_VERIFIER_ADDRESS"
set_persistent_env "GROTH16_VERIFIER_RPC_URL" "$GROTH16_VERIFIER_RPC_URL"

# Reload .bashrc to apply changes
source ~/.bashrc
