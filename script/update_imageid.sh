#!/usr/bin/env bash

# Script to automatically update RISC0 image IDs and SP1 VK hashes in .env file
# by reading from build output
#
# Usage:
#   ./script/update_imageid.sh risc0 [output_file]    # Update RISC0 image IDs from file or temp
#   ./script/update_imageid.sh sp1 [output_file]      # Update SP1 VK hashes from file or temp
#
# This script is automatically called by build.sh after building RISC0 or SP1 provers.
# It extracts the new image IDs/VK hashes from the provided build output and updates the .env file.
#
# If no output_file is provided, it will look for temp files:
#   /tmp/risc0_build_output.txt for RISC0
#   /tmp/sp1_build_output.txt for SP1

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Function to extract RISC0 image ID from build output
extract_risc0_image_id() {
    local build_output="$1"
    local binary_name="$2"
    
    # Look for the pattern "risc0 elf image id: <hex_string>" before the binary path
    local image_id=""
    
    # Find the image ID that appears before the specific binary path
    if [ "$binary_name" = "risc0-aggregation" ]; then
        # Get the image ID that appears before risc0-aggregation path
        image_id=$(echo "$build_output" | grep -B1 "risc0-aggregation" | grep "risc0 elf image id:" | sed 's/.*risc0 elf image id: //' | head -1)
    elif [ "$binary_name" = "risc0-batch" ]; then
        # Get the image ID that appears before risc0-batch path  
        image_id=$(echo "$build_output" | grep -B1 "risc0-batch" | grep "risc0 elf image id:" | sed 's/.*risc0 elf image id: //' | head -1)
    fi
    
    # Fallback: if context search fails, try sequential search based on order
    if [ -z "$image_id" ]; then
        if [ "$binary_name" = "risc0-aggregation" ]; then
            # Get first image ID for aggregation
            image_id=$(echo "$build_output" | grep "risc0 elf image id:" | sed 's/.*risc0 elf image id: //' | head -1)
        elif [ "$binary_name" = "risc0-batch" ]; then
            # Get second image ID for batch
            image_id=$(echo "$build_output" | grep "risc0 elf image id:" | sed 's/.*risc0 elf image id: //' | tail -1)
        fi
    fi
    
    if [ -z "$image_id" ]; then
        print_error "Failed to extract RISC0 image ID for $binary_name"
        return 1
    fi
    
    echo "$image_id"
}

# Function to extract SP1 VK hash from build output
extract_sp1_vk_hash() {
    local build_output="$1"
    local binary_name="$2"
    
    # Look for the pattern "sp1 elf vk hash_bytes is: <hex_string>" in context of the binary
    local vk_hash=""
    
    # Get all lines containing the binary name and nearby VK hash lines
    local context_lines=$(echo "$build_output" | grep -B5 -A5 "$binary_name")
    vk_hash=$(echo "$context_lines" | grep "sp1 elf vk hash_bytes is:" | sed 's/.*sp1 elf vk hash_bytes is: //' | head -1)
    
    # If context search fails, try sequential search based on order
    if [ -z "$vk_hash" ]; then
        if [ "$binary_name" = "sp1-aggregation" ]; then
            # Get first VK hash for aggregation
            vk_hash=$(echo "$build_output" | grep "sp1 elf vk hash_bytes is:" | sed 's/.*sp1 elf vk hash_bytes is: //' | head -1)
        elif [ "$binary_name" = "sp1-batch" ]; then
            # Get second VK hash for batch
            vk_hash=$(echo "$build_output" | grep "sp1 elf vk hash_bytes is:" | sed 's/.*sp1 elf vk hash_bytes is: //' | tail -1)
        fi
    fi
    
    if [ -z "$vk_hash" ]; then
        print_error "Failed to extract SP1 VK hash for $binary_name"
        return 1
    fi
    
    echo "$vk_hash"
}

# Function to update .env file
update_env_file() {
    local env_file=".env"
    local temp_file=".env.tmp"
    
    # Check if .env file exists
    if [ ! -f "$env_file" ]; then
        print_error ".env file not found in current directory"
        return 1
    fi
    
    
    # Read current .env file
    local env_content=$(cat "$env_file")
    
    # Update RISC0 image IDs if provided
    if [ -n "$RISC0_AGGREGATION_ID" ]; then
        env_content=$(echo "$env_content" | sed "s/^RISC0_AGGREGATION_ID=.*/RISC0_AGGREGATION_ID=$RISC0_AGGREGATION_ID/")
        print_status "Updated RISC0_AGGREGATION_ID: $RISC0_AGGREGATION_ID"
    fi
    
    if [ -n "$RISC0_BATCH_ID" ]; then
        env_content=$(echo "$env_content" | sed "s/^RISC0_BATCH_ID=.*/RISC0_BATCH_ID=$RISC0_BATCH_ID/")
        print_status "Updated RISC0_BATCH_ID: $RISC0_BATCH_ID"
    fi
    
    # Update SP1 VK hashes if provided
    if [ -n "$SP1_AGGREGATION_VK_HASH" ]; then
        env_content=$(echo "$env_content" | sed "s/^SP1_AGGREGATION_VK_HASH=.*/SP1_AGGREGATION_VK_HASH=$SP1_AGGREGATION_VK_HASH/")
        print_status "Updated SP1_AGGREGATION_VK_HASH: $SP1_AGGREGATION_VK_HASH"
    fi
    
    if [ -n "$SP1_BATCH_VK_HASH" ]; then
        env_content=$(echo "$env_content" | sed "s/^SP1_BATCH_VK_HASH=.*/SP1_BATCH_VK_HASH=$SP1_BATCH_VK_HASH/")
        print_status "Updated SP1_BATCH_VK_HASH: $SP1_BATCH_VK_HASH"
    fi
    
    # Write updated content to .env file
    echo "$env_content" > "$env_file"
    print_status "Successfully updated .env file"
}

# Function to extract RISC0 image IDs from build output file or stdin
extract_risc0_ids_from_output() {
    local build_output=""
    
    # Read from file if provided, otherwise from stdin
    if [ -n "$1" ] && [ -f "$1" ]; then
        build_output=$(cat "$1")
        print_status "Reading RISC0 build output from file: $1"
    else
        # Try to read the latest build output from a temp file if it exists
        local temp_file="/tmp/risc0_build_output.txt"
        if [ -f "$temp_file" ]; then
            build_output=$(cat "$temp_file")
            print_status "Reading RISC0 build output from temp file: $temp_file"
        else
            print_error "No RISC0 build output available. Please run the RISC0 builder first."
            return 1
        fi
    fi
    
    # Extract image IDs
    local aggregation_id=$(extract_risc0_image_id "$build_output" "risc0-aggregation")
    local batch_id=$(extract_risc0_image_id "$build_output" "risc0-batch")
    
    if [ -n "$aggregation_id" ] && [ -n "$batch_id" ]; then
        RISC0_AGGREGATION_ID="$aggregation_id"
        RISC0_BATCH_ID="$batch_id"
        print_status "Extracted RISC0 image IDs:"
        print_status "  Aggregation: $aggregation_id"
        print_status "  Batch: $batch_id"
    else
        print_error "Failed to extract RISC0 image IDs from build output"
        return 1
    fi
}

# Function to extract SP1 VK hashes from build output file or stdin
extract_sp1_hashes_from_output() {
    local build_output=""
    
    # Read from file if provided, otherwise from stdin
    if [ -n "$1" ] && [ -f "$1" ]; then
        build_output=$(cat "$1")
        print_status "Reading SP1 build output from file: $1"
    else
        # Try to read the latest build output from a temp file if it exists
        local temp_file="/tmp/sp1_build_output.txt"
        if [ -f "$temp_file" ]; then
            build_output=$(cat "$temp_file")
            print_status "Reading SP1 build output from temp file: $temp_file"
        else
            print_error "No SP1 build output available. Please run the SP1 builder first."
            return 1
        fi
    fi
    
    # Extract VK hashes
    local aggregation_vk_hash=$(extract_sp1_vk_hash "$build_output" "sp1-aggregation")
    local batch_vk_hash=$(extract_sp1_vk_hash "$build_output" "sp1-batch")
    
    if [ -n "$aggregation_vk_hash" ] && [ -n "$batch_vk_hash" ]; then
        SP1_AGGREGATION_VK_HASH="$aggregation_vk_hash"
        SP1_BATCH_VK_HASH="$batch_vk_hash"
        print_status "Extracted SP1 VK hashes:"
        print_status "  Aggregation: $aggregation_vk_hash"
        print_status "  Batch: $batch_vk_hash"
    else
        print_error "Failed to extract SP1 VK hashes from build output"
        return 1
    fi
}

# Main function
main() {
    print_status "Starting automatic environment update..."
    
    # Initialize variables
    RISC0_AGGREGATION_ID=""
    RISC0_BATCH_ID=""
    SP1_AGGREGATION_VK_HASH=""
    SP1_BATCH_VK_HASH=""
    
    # Check if we're in the right directory
    if [ ! -f "Cargo.toml" ]; then
        print_error "This script must be run from the project root directory"
        exit 1
    fi
    
    # Parse command line arguments
    local mode=""
    if [ $# -gt 0 ]; then
        case "$1" in
            "risc0")
                mode="risc0"
                ;;
            "sp1")
                mode="sp1"
                ;;
            *)
                print_error "Unknown mode: $1. Use 'risc0' or 'sp1'"
                exit 1
                ;;
        esac
    else
        print_error "Mode must be specified. Use 'risc0' or 'sp1'"
        exit 1
    fi
    
    # Extract RISC0 image IDs from output
    if [ "$mode" = "risc0" ]; then
        if extract_risc0_ids_from_output "$2"; then
            print_status "RISC0 image IDs extracted successfully"
        else
            print_error "Failed to extract RISC0 image IDs"
            exit 1
        fi
    fi
    
    # Extract SP1 VK hashes from output
    if [ "$mode" = "sp1" ]; then
        if extract_sp1_hashes_from_output "$2"; then
            print_status "SP1 VK hashes extracted successfully"
        else
            print_error "Failed to extract SP1 VK hashes"
            exit 1
        fi
    fi
    
    # Update .env file
    if update_env_file; then
        print_status "Environment file updated successfully"
    else
        print_error "Failed to update .env file"
        exit 1
    fi
    
    print_status "Automatic environment update completed successfully!"
}

# Run main function
main "$@" 