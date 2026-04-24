#!/usr/bin/env bash

# Environment variables
# risc0
BONSAI_API_KEY=$BONSAI_API_KEY
BONSAI_API_URL=$BONSAI_API_URL
RAIKO_AGENT_URL=${RAIKO_AGENT_URL:-http://localhost:9999/proof}
RAIKO_AGENT_API_KEY=$RAIKO_AGENT_API_KEY
# reference verifier was deployed in holesky
GROTH16_VERIFIER_RPC_URL=https://ethereum-hoodi-rpc.publicnode.com
GROTH16_VERIFIER_ADDRESS=0x2a098988600d87650Fb061FfAff08B97149Fa84D #RiscZeroGroth16Verifier

# sp1
SP1_PROVER=network
SKIP_SIMULATION=true
PROVER_NETWORK_RPC=$PROVER_NETWORK_RPC
SP1_PRIVATE_KEY=$SP1_PRIVATE_KEY
SP1_VERIFIER_RPC_URL=https://ethereum-hoodi-rpc.publicnode.com
SP1_VERIFIER_ADDRESS=0x2a5A70409Ee9F057503a50E0F4614A6d8CcBb462 #SP1PlonkVerifier

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
set_persistent_env "RAIKO_AGENT_URL" "$RAIKO_AGENT_URL"
set_persistent_env "GROTH16_VERIFIER_ADDRESS" "$GROTH16_VERIFIER_ADDRESS"
set_persistent_env "GROTH16_VERIFIER_RPC_URL" "$GROTH16_VERIFIER_RPC_URL"

# Reload .bashrc to apply changes
source ~/.bashrc
