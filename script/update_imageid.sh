#!/usr/bin/env bash

# Script to automatically update RISC0 image IDs, SP1 VK hashes, Zisk image IDs, SGX MRENCLAVE, and SGXGETH MRENCLAVE in .env file
# by reading from build output or extracting MRENCLAVE directly
#
# Usage:
#   ./script/update_imageid.sh risc0 [output_file]    # Update RISC0 image IDs from file or temp
#   ./script/update_imageid.sh sp1 [output_file]      # Update SP1 VK hashes from file or temp
#   ./script/update_imageid.sh zisk                   # Set default Zisk image IDs
#   ./script/update_imageid.sh sgx                    # Extract and update SGX MRENCLAVE
#   ./script/update_imageid.sh sgxgeth                # Extract and update SGXGETH MRENCLAVE
#
# This script is automatically called by build.sh after building RISC0, SP1, SGX, or SGXGETH provers.
# It extracts the new image IDs/VK hashes/MRENCLAVE from the provided build output and updates the .env file.
#
# If no output_file is provided, it will look for temp files:
#   /tmp/risc0_build_output.txt for RISC0
#   /tmp/sp1_build_output.txt for SP1
# For SGX, it directly builds and extracts the MRENCLAVE using Gramine tools.
# For SGXGETH, it directly builds and extracts the MRENCLAVE using EGO tools.

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
    
    # Extract VK hash based on binary order (aggregation first, batch second)
    local vk_hash=""
    if [ "$binary_name" = "sp1-aggregation" ]; then
        vk_hash=$(echo "$build_output" | grep "sp1 elf vk hash_bytes is:" | sed 's/.*sp1 elf vk hash_bytes is: //' | head -1)
    elif [ "$binary_name" = "sp1-batch" ]; then
        vk_hash=$(echo "$build_output" | grep "sp1 elf vk hash_bytes is:" | sed 's/.*sp1 elf vk hash_bytes is: //' | tail -1)
    fi
    
    if [ -z "$vk_hash" ]; then
        print_error "Failed to extract SP1 VK hash for $binary_name"
        return 1
    fi
    
    echo "$vk_hash"
}

# Function to set default Zisk image IDs
set_zisk_default_ids() {
    print_status "Setting default Zisk image IDs for consistency with other zkVMs"
    
    # Set default values - these can be updated in the future when Zisk implements native image IDs
    ZISK_AGGREGATION_ID="0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    ZISK_BATCH_ID="0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    
    print_status "Using default Zisk image IDs:"
    print_status "  Aggregation: $ZISK_AGGREGATION_ID"
    print_status "  Batch: $ZISK_BATCH_ID"
}

# Function to check if Gramine tools are available
check_gramine_tools() {
    if ! command -v gramine-manifest &> /dev/null; then
        return 1
    fi
    if ! command -v gramine-sgx-sign &> /dev/null; then
        return 1
    fi
    if ! command -v gramine-sgx-sigstruct-view &> /dev/null; then
        return 1
    fi
    return 0
}

# Function to check if EGO tools are available
check_ego_tools() {
    if ! command -v ego &> /dev/null; then
        return 1
    fi
    return 0
}

# Function to extract MRENCLAVE from SGX runtime quote
extract_sgx_mrenclave() {
    print_status "Building SGX guest and extracting MRENCLAVE from runtime quote..."
    
    # Check if Gramine tools are available
    if ! check_gramine_tools; then
        print_warning "Gramine tools not found on host system!"
        print_warning "Skipping local MRENCLAVE extraction - Docker build will handle SGX signing"
        return 0
    fi
    
    # Navigate to SGX guest directory
    cd provers/sgx/guest
    
    # Check if SGX guest binary exists
    if [ ! -f "../../../target/release/sgx-guest" ]; then
        print_warning "SGX guest binary not found at ../../../target/release/sgx-guest"
        print_status "This might be because:"
        echo "  - SGX features are not built yet (run: cargo build --release --features sgx)"
        echo "  - Building in Docker context where paths are different"
        print_status "Skipping local MRENCLAVE extraction"
        return 0
    fi
    
    # Copy the binary to current directory for Gramine processing
    if ! cp ../../../target/release/sgx-guest .; then
        print_error "Failed to copy SGX guest binary"
        return 1
    fi
    
    # Generate Gramine manifest
    if ! gramine-manifest \
        -Dlog_level=error \
        -Ddirect_mode=0 \
        -Darch_libdir=/lib/x86_64-linux-gnu/ \
        ../config/sgx-guest.local.manifest.template \
        sgx-guest.manifest; then
        print_error "Failed to generate Gramine manifest"
        return 1
    fi
    
    # Sign the manifest to generate SGX signature using project's enclave key
    if ! gramine-sgx-sign \
        --manifest sgx-guest.manifest \
        --output sgx-guest.manifest.sgx \
        --key ../../../docker/enclave-key.pem; then
        print_error "Failed to sign SGX manifest"
        return 1
    fi
    
    # Extract MRENCLAVE from signature structure  
    print_status "Extracting MRENCLAVE from signed manifest..."
    local SIGSTRUCT_OUTPUT
    SIGSTRUCT_OUTPUT=$(gramine-sgx-sigstruct-view sgx-guest.sig 2>&1)
    
    print_status "Sigstruct output preview:"
    echo "$SIGSTRUCT_OUTPUT" | head -10
    
    # Extract the MRENCLAVE value from "mr_enclave:" line
    local MRENCLAVE_OUTPUT
    MRENCLAVE_OUTPUT=$(echo "$SIGSTRUCT_OUTPUT" | grep "mr_enclave:" | grep -o '[a-fA-F0-9]\{64\}' | head -1)
    
    print_status "Raw MRENCLAVE output: '$MRENCLAVE_OUTPUT' (length: ${#MRENCLAVE_OUTPUT})"
    
    if [ -n "$MRENCLAVE_OUTPUT" ] && [ ${#MRENCLAVE_OUTPUT} -eq 64 ]; then
        print_status "Extracted runtime MRENCLAVE: $MRENCLAVE_OUTPUT"
        
        # Clean up temporary files
        print_status "Cleaning up temporary files..."
        rm -f sgx-guest sgx-guest.manifest sgx-guest.manifest.sgx sgx-guest.sig
        
        # Navigate back to root directory
        cd ../../..
        
        # Update .env file with extracted MRENCLAVE
        update_env_mrenclave "$MRENCLAVE_OUTPUT"
    else
        print_error "Failed to extract MRENCLAVE from SGX runtime quote"
        print_error "Expected 64-character hex string, got: '$MRENCLAVE_OUTPUT'"
        
        # Clean up temporary files even on failure
        print_status "Cleaning up temporary files..."
        rm -f sgx-guest sgx-guest.manifest sgx-guest.manifest.sgx sgx-guest.sig
        
        cd ../../..
        return 1
    fi
}

# Function to extract SGX MRENCLAVE from build log files
extract_sgx_mrenclave_from_output() {
    print_status "Extracting SGX MRENCLAVE from build output..."
    
    local build_output=""
    local log_file=""
    
    # Read from file if provided
    if [ -n "$1" ] && [ -f "$1" ]; then
        log_file="$1"
        build_output=$(cat "$1")
        print_status "Reading SGX build output from file: $1"
    else
        print_error "No SGX build log file provided"
        return 1
    fi
    
    # Extract MRENCLAVE from build log
    # Look for the pattern "mr_enclave:" which appears in the Gramine sigstruct output
    local MRENCLAVE_OUTPUT
    MRENCLAVE_OUTPUT=$(echo "$build_output" | grep "mr_enclave:" | grep -o '[a-fA-F0-9]\{64\}' | head -1)
    
    if [ -n "$MRENCLAVE_OUTPUT" ] && [ ${#MRENCLAVE_OUTPUT} -eq 64 ]; then
        print_status "Extracted SGX_MRENCLAVE from build log: $MRENCLAVE_OUTPUT"
        
        # Update .env file with extracted MRENCLAVE
        update_env_mrenclave "$MRENCLAVE_OUTPUT"
    else
        print_error "Failed to extract SGX MRENCLAVE from build log"
        if [ -n "$log_file" ]; then
            print_error "Searched in log file: $log_file"
        fi
        print_error "Expected 64-character hex string, got: '$MRENCLAVE_OUTPUT'"
        return 1
    fi
}

# Function to update .env file with MRENCLAVE value
update_env_mrenclave() {
    local MRENCLAVE=$1
    local ENV_FILE=".env"
    
    # Check if file exists, create if not
    if [ ! -f "$ENV_FILE" ]; then
        print_status "Creating .env file..."
        touch "$ENV_FILE"
    fi
    
    # Update or add SGX_MRENCLAVE in the file
    if grep -q "^SGX_MRENCLAVE=" "$ENV_FILE"; then
        # Update existing entry
        sed -i "s/^SGX_MRENCLAVE=.*/SGX_MRENCLAVE=$MRENCLAVE/" "$ENV_FILE"
        print_status "Updated SGX_MRENCLAVE in $ENV_FILE: $MRENCLAVE"
    else
        # Add new entry
        echo "SGX_MRENCLAVE=$MRENCLAVE" >> "$ENV_FILE"
        print_status "Added SGX_MRENCLAVE to $ENV_FILE: $MRENCLAVE"
    fi
}

# Function to extract SGXGETH MRENCLAVE from build log files
extract_sgxgeth_mrenclave_from_output() {
    print_status "Extracting SGXGETH MRENCLAVE from build output..."
    
    local build_output=""
    local log_file=""
    
    # Read from file if provided
    if [ -n "$1" ] && [ -f "$1" ]; then
        log_file="$1"
        build_output=$(cat "$1")
        print_status "Reading SGXGETH build output from file: $1"
    else
        # Try to find the latest log.build.raiko.* file
        log_file=$(ls -t log.build.raiko.* 2>/dev/null | head -1)
        if [ -n "$log_file" ] && [ -f "$log_file" ]; then
            build_output=$(cat "$log_file")
            print_status "Reading SGXGETH build output from latest log file: $log_file"
        else
            print_error "No SGXGETH build log available. Please run 'script/publish-image.sh' with tee option first."
            print_error "Expected log files matching pattern: log.build.raiko.*"
            return 1
        fi
    fi
    
    # Check if ego uniqueid step was cached (no actual uniqueid generated)
    if echo "$build_output" | grep -A 1 "RUN ego uniqueid" | grep -q "CACHED"; then
        print_warning "ego uniqueid step was cached - no new uniqueid generated"
        print_status "Using default SGXGETH_MRENCLAVE value for cached build"
        local DEFAULT_MRENCLAVE="ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        
        # Update .env file with default MRENCLAVE
        update_env_sgxgeth_mrenclave "$DEFAULT_MRENCLAVE"
        return 0
    fi
    
    # Extract MRENCLAVE from build log
    # Look for the uniqueid after "RUN ego uniqueid" command in EGO build output
    # The uniqueid appears on a line by itself after the EGo version info
    local MRENCLAVE_OUTPUT
    MRENCLAVE_OUTPUT=$(echo "$build_output" | awk '/RUN ego uniqueid/{getline; getline; if(/^#[0-9]+ [0-9]+\.[0-9]+ [a-fA-F0-9]{64}$/) {gsub(/^#[0-9]+ [0-9]+\.[0-9]+ /, ""); print}}' | head -1)
    
    # If the structured search fails, try to find any 64-char hex string after "ego uniqueid"
    if [ -z "$MRENCLAVE_OUTPUT" ]; then
        MRENCLAVE_OUTPUT=$(echo "$build_output" | sed -n '/RUN ego uniqueid/,+5p' | grep -o '[a-fA-F0-9]\{64\}' | head -1)
    fi
    
    # If both pattern searches fail, try general hex pattern search
    if [ -z "$MRENCLAVE_OUTPUT" ]; then
        MRENCLAVE_OUTPUT=$(echo "$build_output" | grep -o '[a-fA-F0-9]\{64\}' | head -1)
    fi
    
    if [ -n "$MRENCLAVE_OUTPUT" ] && [ ${#MRENCLAVE_OUTPUT} -eq 64 ]; then
        print_status "Extracted SGXGETH_MRENCLAVE from build log: $MRENCLAVE_OUTPUT"
        
        # Update .env file with extracted MRENCLAVE
        update_env_sgxgeth_mrenclave "$MRENCLAVE_OUTPUT"
    else
        print_error "Failed to extract SGXGETH MRENCLAVE from build log"
        if [ -n "$log_file" ]; then
            print_error "Searched in log file: $log_file"
        fi
        print_error "Expected 64-character hex string, got: '$MRENCLAVE_OUTPUT'"
        return 1
    fi
}


# Function to update .env file with SGXGETH_MRENCLAVE value
update_env_sgxgeth_mrenclave() {
    local MRENCLAVE=$1
    local ENV_FILE=".env"
    
    # Check if file exists, create if not
    if [ ! -f "$ENV_FILE" ]; then
        print_status "Creating .env file..."
        touch "$ENV_FILE"
    fi
    
    # Update or add SGXGETH_MRENCLAVE in the file
    if grep -q "^SGXGETH_MRENCLAVE=" "$ENV_FILE"; then
        # Update existing entry
        sed -i "s/^SGXGETH_MRENCLAVE=.*/SGXGETH_MRENCLAVE=$MRENCLAVE/" "$ENV_FILE"
        print_status "Updated SGXGETH_MRENCLAVE in $ENV_FILE: $MRENCLAVE"
    else
        # Add new entry
        echo "SGXGETH_MRENCLAVE=$MRENCLAVE" >> "$ENV_FILE"
        print_status "Added SGXGETH_MRENCLAVE to $ENV_FILE: $MRENCLAVE"
    fi
}

# Function to update .env file
update_env_file() {
    local env_file=".env"
    
    # Check if file exists
    if [ ! -f "$env_file" ]; then
        print_error ".env file not found in current directory"
        return 1
    fi
    
    # Read current file content
    local env_content=$(cat "$env_file")
    
    # Update RISC0 image IDs if provided
    if [ -n "$RISC0_AGGREGATION_ID" ]; then
        env_content=$(echo "$env_content" | sed "s/^RISC0_AGGREGATION_ID=.*/RISC0_AGGREGATION_ID=$RISC0_AGGREGATION_ID/")
        print_status "Updated RISC0_AGGREGATION_ID in $env_file: $RISC0_AGGREGATION_ID"
    fi
    
    if [ -n "$RISC0_BATCH_ID" ]; then
        env_content=$(echo "$env_content" | sed "s/^RISC0_BATCH_ID=.*/RISC0_BATCH_ID=$RISC0_BATCH_ID/")
        print_status "Updated RISC0_BATCH_ID in $env_file: $RISC0_BATCH_ID"
    fi
    
    # Update SP1 VK hashes if provided
    if [ -n "$SP1_AGGREGATION_VK_HASH" ]; then
        env_content=$(echo "$env_content" | sed "s/^SP1_AGGREGATION_VK_HASH=.*/SP1_AGGREGATION_VK_HASH=$SP1_AGGREGATION_VK_HASH/")
        print_status "Updated SP1_AGGREGATION_VK_HASH in $env_file: $SP1_AGGREGATION_VK_HASH"
    fi
    
    if [ -n "$SP1_BATCH_VK_HASH" ]; then
        env_content=$(echo "$env_content" | sed "s/^SP1_BATCH_VK_HASH=.*/SP1_BATCH_VK_HASH=$SP1_BATCH_VK_HASH/")
        print_status "Updated SP1_BATCH_VK_HASH in $env_file: $SP1_BATCH_VK_HASH"
    fi
    
    # Update Zisk image IDs if provided
    if [ -n "$ZISK_AGGREGATION_ID" ]; then
        env_content=$(echo "$env_content" | sed "s/^ZISK_AGGREGATION_ID=.*/ZISK_AGGREGATION_ID=$ZISK_AGGREGATION_ID/")
        print_status "Updated ZISK_AGGREGATION_ID in $env_file: $ZISK_AGGREGATION_ID"
    fi
    
    if [ -n "$ZISK_BATCH_ID" ]; then
        env_content=$(echo "$env_content" | sed "s/^ZISK_BATCH_ID=.*/ZISK_BATCH_ID=$ZISK_BATCH_ID/")
        print_status "Updated ZISK_BATCH_ID in $env_file: $ZISK_BATCH_ID"
    fi
    
    # Write updated content to file
    echo "$env_content" > "$env_file"
    print_status "Successfully updated $env_file"
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
    ZISK_AGGREGATION_ID=""
    ZISK_BATCH_ID=""
    
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
            "zisk")
                mode="zisk"
                ;;
            "sgx")
                mode="sgx"
                ;;
            "sgxgeth")
                mode="sgxgeth"
                ;;
            *)
                print_error "Unknown mode: $1. Use 'risc0', 'sp1', 'zisk', 'sgx', or 'sgxgeth'"
                exit 1
                ;;
        esac
    else
        print_error "Mode must be specified. Use 'risc0', 'sp1', 'zisk', 'sgx', or 'sgxgeth'"
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
    
    # Set default Zisk image IDs
    if [ "$mode" = "zisk" ]; then
        set_zisk_default_ids
        print_status "Zisk image IDs set successfully"
    fi
    
    # Extract SGX MRENCLAVE
    if [ "$mode" = "sgx" ]; then
        # If a build log file is provided, extract from it (Docker build scenario)
        if [ -n "$2" ] && [ -f "$2" ]; then
            if extract_sgx_mrenclave_from_output "$2"; then
                print_status "SGX MRENCLAVE extracted successfully"
            else
                print_error "Failed to extract SGX MRENCLAVE from build log"
                exit 1
            fi
        else
            # Local build scenario - try to build and extract locally
            if extract_sgx_mrenclave; then
                print_status "SGX MRENCLAVE extracted successfully"
            else
                print_error "Failed to extract SGX MRENCLAVE"
                exit 1
            fi
        fi
    fi
    
    # Extract SGXGETH MRENCLAVE from Docker build output
    if [ "$mode" = "sgxgeth" ]; then
        if extract_sgxgeth_mrenclave_from_output "$2"; then
            print_status "SGXGETH MRENCLAVE extracted successfully"
        else
            print_error "Failed to extract SGXGETH MRENCLAVE"
            exit 1
        fi
    fi
    
    # Update .env file (only for risc0, sp1, and zisk modes, sgx and sgxgeth handle their own .env updates)
    if [ "$mode" != "sgx" ] && [ "$mode" != "sgxgeth" ]; then
        if update_env_file; then
            print_status "Environment file updated successfully"
        else
            print_error "Failed to update .env file"
            exit 1
        fi
    fi
    
    print_status "Automatic environment update completed successfully!"
}

# Run main function
main "$@" 