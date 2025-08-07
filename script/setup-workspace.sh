#!/usr/bin/env bash

# Setup workspace members based on target
set -e

TARGET=${1:-native}
CARGO_TOML="Cargo.toml"
BACKUP_TOML="Cargo.toml.backup"

# Backup original Cargo.toml if not already backed up
if [ ! -f "$BACKUP_TOML" ]; then
    cp "$CARGO_TOML" "$BACKUP_TOML"
fi

# Restore from backup
cp "$BACKUP_TOML" "$CARGO_TOML"

# Function to add conditional members to workspace
add_conditional_members() {
    local target=$1
    local temp_file=$(mktemp)
    
    # Read the conditional members for this target from metadata
    case $target in
        "sp1")
            MEMBERS='    "provers/sp1/driver",
    "provers/sp1/builder",'
            ;;
        "risc0")  
            MEMBERS='    "provers/risc0/driver",
    "provers/risc0/builder",'
            ;;
        "zisk")
            MEMBERS='    "provers/zisk/driver",
    "provers/zisk/builder",'
            ;;
        "sgx")
            MEMBERS='    "provers/sgx/prover",
    "provers/sgx/guest", 
    "provers/sgx/setup",'
            ;;
        "native"|*)
            # For native or unknown targets, add all members
            MEMBERS='    "provers/sp1/driver",
    "provers/sp1/builder",
    "provers/risc0/driver",
    "provers/risc0/builder", 
    "provers/zisk/driver",
    "provers/zisk/builder",
    "provers/sgx/prover",
    "provers/sgx/guest",
    "provers/sgx/setup",'
            ;;
    esac
    
    # Insert the conditional members before the closing bracket
    awk -v members="$MEMBERS" '
    /^]$/ && in_workspace_members {
        print members
        print $0
        in_workspace_members = 0
        next
    }
    /^members = \[/ {
        in_workspace_members = 1
    }
    { print }
    ' "$CARGO_TOML" > "$temp_file"
    
    mv "$temp_file" "$CARGO_TOML"
}

if [ "$TARGET" != "native" ] && [ "$TARGET" != "" ]; then
    add_conditional_members "$TARGET"
    echo "Added $TARGET-specific members to workspace"
else 
    echo "Using default workspace members (all provers)"
fi

echo "Workspace setup complete"