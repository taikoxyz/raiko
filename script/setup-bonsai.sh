#!/usr/bin/env bash

# Environment variables
BONSAI_API_KEY="1234"
BONSAI_API_URL="https://api.bonsai.xyzz/"
GROTH16_VERIFIER_ADDRESS="850EC3780CeDfdb116E38B009d0bf7a1ef1b8b38"
GROTH16_VERIFIER_RPC_URL="https://sepolia.infura.io/v3/4c76691f5f384d30bed910018c28ba1d"

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
