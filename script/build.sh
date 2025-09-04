#!/usr/bin/env bash

# Any error will result in failure
set -e

# ZISK Agent Mode: ZISK runs as an isolated microservice agent
# - Completely isolated from SP1/RISC0 dependencies 
# - Agent runs on port 9998 by default
# - GPU support automatically enabled when CUDA toolkit is detected
# 
# Prerequisites for GPU support:
# - NVIDIA GPU  
# - CUDA Toolkit installed (https://developer.nvidia.com/cuda-toolkit)
# - Build on the target GPU server for optimal performance

TOOLCHAIN_RISC0=+nightly-2024-12-20
TOOLCHAIN_SP1=+nightly-2024-12-20
TOOLCHAIN_SGX=+nightly-2024-12-20
TOOLCHAIN_ZISK=+nightly-2024-12-20

check_toolchain() {
    local TOOLCHAIN=$1

    # Remove the plus sign from the toolchain name
    TOOLCHAIN=${TOOLCHAIN#+}

    # Skip rustup check if rustup is not available (e.g., using Zisk's Rust toolchain)
    if ! command -v rustup &> /dev/null; then
        echo "rustup not found, skipping toolchain check (using alternative Rust installation)"
        return 0
    fi

    # Function to check if the toolchain is installed
    exist() {
        rustup toolchain list | grep "$TOOLCHAIN" >/dev/null
    }

    # Main script logic
    if exist; then
        echo "Toolchain $TOOLCHAIN exists"
    else
        echo "Installing Rust toolchain: $TOOLCHAIN"
        rustup install "$TOOLCHAIN"
    fi
}

if [ -z "${DEBUG}" ]; then
    FLAGS=--release
else
    echo "Warning: in debug mode"
fi

if [ -z "${RUN}" ]; then
    COMMAND=build
else
    COMMAND=run
fi

if [ "$CPU_OPT" = "1" ]; then
    export RUSTFLAGS='-C target-cpu=native'
    echo "Enable cpu optimization with host RUSTFLAGS"
fi

# NATIVE
if [ -z "$1" ] || [ "$1" == "native" ]; then
    if [ -n "${CLIPPY}" ]; then
        cargo clippy -- -D warnings
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building native prover"
            cargo build ${FLAGS}
        else
            echo "Building native tests"
            cargo test ${FLAGS} --no-run
        fi
    else
        if [ -z "${TEST}" ]; then
            echo "Running native prover"
            cargo run ${FLAGS}
        else
            echo "Running native tests"
            cargo test ${FLAGS}
        fi
    fi
fi

# SGX
if [ "$1" == "sgx" ]; then
    check_toolchain $TOOLCHAIN_SGX
    if [ "$MOCK" = "1" ]; then
        export SGX_DIRECT=1
        echo "SGX_DIRECT is set to $SGX_DIRECT"
    fi
    if [ -n "${CLIPPY}" ]; then
        cargo ${TOOLCHAIN_SGX} clippy -p raiko-host -p sgx-prover -F "sgx enable" -- -D warnings
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building SGX prover"
            cargo ${TOOLCHAIN_SGX} build ${FLAGS} --features sgx
            
            # Extract MRENCLAVE after successful build
            echo "Extracting MRENCLAVE from SGX build..."
            # Check multiple indicators that we're in a container/CI environment
            if [ -f "/.dockerenv" ] || [ -n "${DOCKER_BUILDKIT}" ] || [ -n "${CI}" ] || [ ! -f ".env" ] || grep -q docker /proc/1/cgroup 2>/dev/null; then
                echo "Container/CI build detected, skipping MRENCLAVE .env update (will be handled by publish-image.sh)"
            else
                ./script/update_imageid.sh sgx
            fi
        else
            echo "Building SGX tests"
            cargo ${TOOLCHAIN_SGX} test ${FLAGS} -p raiko-host -p sgx-prover --features "sgx enable" --no-run
        fi
    else
        if [ -z "${TEST}" ]; then
            echo "Running SGX prover"
            cargo ${TOOLCHAIN_SGX} run ${FLAGS} --features sgx
        else
            echo "Running SGX tests"
            cargo ${TOOLCHAIN_SGX} test ${FLAGS} -p raiko-host -p sgx-prover --features "sgx enable"
        fi
    fi
fi


# RISC0
if [ "$1" == "risc0" ]; then
    check_toolchain $TOOLCHAIN_RISC0
    ./script/setup-bonsai.sh
    if [ "$MOCK" = "1" ]; then
        export RISC0_DEV_MODE=1
        echo "RISC0_DEV_MODE is set to $RISC0_DEV_MODE"
    fi
    if [ -n "${CLIPPY}" ]; then
        MOCK=1
        RISC0_DEV_MODE=1
        CI=1
        cargo ${TOOLCHAIN_RISC0} run --manifest-path provers/risc0/builder/Cargo.toml --bin risc0-builder
        cargo ${TOOLCHAIN_RISC0} clippy -p raiko-host -F risc0
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building Risc0 prover"
            cargo ${TOOLCHAIN_RISC0} run --manifest-path provers/risc0/builder/Cargo.toml --bin risc0-builder 2>&1 | tee /tmp/risc0_build_output.txt
            # Skip updating .env during Docker builds (no .env file exists in container)  
            # The publish-image.sh script will update the local .env file after the build
            # Check multiple indicators that we're in a container/CI environment
            if [ -f "/.dockerenv" ] || [ -n "${DOCKER_BUILDKIT}" ] || [ -n "${CI}" ] || [ ! -f ".env" ] || grep -q docker /proc/1/cgroup 2>/dev/null; then
                echo "Container/CI build detected, skipping .env update (will be handled by publish-image.sh)"
            else
                echo "Updating environment with new RISC0 image IDs..."
                ./script/update_imageid.sh risc0
            fi
        else
            echo "Building test elfs for Risc0 prover"
            cargo ${TOOLCHAIN_RISC0} run --manifest-path provers/risc0/builder/Cargo.toml --bin risc0-builder --no-default-features --features risc0,test,bench
        fi
        if [ -z "${GUEST}" ]; then
            # Clear RISC-V CC environment variables for host build
            unset CC TARGET_CC
            cargo ${TOOLCHAIN_RISC0} build ${FLAGS} --no-default-features --features risc0 --package raiko-host --package raiko-pipeline --package raiko-core
        fi
    else
        if [ -z "${TEST}" ]; then
            echo "Running Risc0 prover"
            # Clear RISC-V CC environment variables for host run
            unset CC TARGET_CC
            cargo ${TOOLCHAIN_RISC0} run ${FLAGS} --no-default-features --features risc0
        else
            echo "Running Risc0 tests"
            cargo ${TOOLCHAIN_RISC0} test ${FLAGS} --lib risc0-driver --no-default-features --features risc0  -- run_unittest_elf
            cargo ${TOOLCHAIN_RISC0} test ${FLAGS} -p raiko-host -p risc0-driver --no-default-features --features "risc0,enable"
        fi
    fi
fi

# SP1
if [ "$1" == "sp1" ]; then
    # Check for C compiler (required for secp256k1-sys in SP1 guest)
    if ! command -v clang &> /dev/null && ! command -v gcc &> /dev/null; then
        echo "Error: No C compiler found. SP1 requires clang or gcc for building secp256k1-sys."
        echo "Please install one of the following:"
        echo "  - Ubuntu/Debian: sudo apt install clang"
        echo "  - Or install GCC: sudo apt install build-essential"
        echo "  - Or set CC environment variable: export CC=gcc (if gcc is installed elsewhere)"
        exit 1
    fi
    
    check_toolchain $TOOLCHAIN_SP1
    if [ "$MOCK" = "1" ]; then
        export SP1_PROVER=mock
        echo "SP1_PROVER is set to $SP1_PROVER"
    fi
    if [ -n "${CLIPPY}" ]; then
        cargo ${TOOLCHAIN_SP1} clippy -p raiko-host -F "sp1,enable"
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building Sp1 prover"
            # Clear RISC-V CC environment variables for SP1 builder
            unset CC TARGET_CC
            cargo ${TOOLCHAIN_SP1} run --manifest-path provers/sp1/builder/Cargo.toml --bin sp1-builder 2>&1 | tee /tmp/sp1_build_output.txt
            # Skip updating .env during Docker builds (no .env file exists in container)
            # The publish-image.sh script will update the local .env file after the build
            # Check multiple indicators that we're in a container/CI environment
            if [ -f "/.dockerenv" ] || [ -n "${DOCKER_BUILDKIT}" ] || [ -n "${CI}" ] || [ ! -f ".env" ] || grep -q docker /proc/1/cgroup 2>/dev/null; then
                echo "Container/CI build detected, skipping .env update (will be handled by publish-image.sh)"
            else
                echo "Updating environment with new SP1 VK hashes..."
                ./script/update_imageid.sh sp1
            fi
        else
            echo "Building test elfs for Sp1 prover"
            cargo ${TOOLCHAIN_SP1} run --manifest-path provers/sp1/builder/Cargo.toml --bin sp1-builder --no-default-features --features sp1,test,bench
        fi
        if [ -z "${GUEST}" ]; then
            echo "Building 'cargo ${TOOLCHAIN_SP1} build ${FLAGS} --no-default-features --features sp1'"
            # Clear RISC-V CC environment variables for host build
            unset CC TARGET_CC
            cargo ${TOOLCHAIN_SP1} build ${FLAGS} --no-default-features --features sp1 --package raiko-host --package raiko-pipeline --package raiko-core
        fi
    else
        if [ -z "${TEST}" ]; then
            echo "Running Sp1 prover"
            # Clear RISC-V CC environment variables for host run
            unset CC TARGET_CC
            cargo ${TOOLCHAIN_SP1} run ${FLAGS} --no-default-features --features sp1
        else
            echo "Running Sp1 unit tests"
            # cargo ${TOOLCHAIN_SP1} test ${FLAGS} --lib sp1-driver --features sp1 -- run_unittest_elf
            cargo ${TOOLCHAIN_SP1} test ${FLAGS} -p raiko-host -p sp1-driver --no-default-features --features "sp1,enable"

            # Don't want to span Succinct Network and wait 2 hours in CI
            # echo "Running Sp1 verification"
            # cargo ${TOOLCHAIN_SP1} run ${FLAGS} --bin sp1-verifier --features enable,sp1-verifier
        fi
    fi
fi

# ZISK Agent Mode
if [ "$1" == "zisk" ]; then
    echo "=== ZISK Agent Mode Build ==="
    echo "Building ZISK as isolated microservice agent"
    
    # Clear any RISC-V related environment variables that might interfere
    unset CC TARGET_CC
    
    # Navigate to ZISK agent directory
    ZISK_AGENT_DIR="provers/zisk/agent"
    if [ ! -d "$ZISK_AGENT_DIR" ]; then
        echo "Error: ZISK agent directory not found at $ZISK_AGENT_DIR"
        exit 1
    fi
    
    echo "Using consolidated ZISK agent at: $ZISK_AGENT_DIR"
    
    if [ -n "${CLIPPY}" ]; then
        echo "Running clippy on ZISK agent workspace..."
        (cd "$ZISK_AGENT_DIR" && cargo clippy --workspace --all-targets --all-features)
        
        # Also run clippy on core integration
        cargo ${TOOLCHAIN_ZISK} clippy -p raiko-core -F "zisk,enable"
        
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building ZISK agent components..."
            
            # Use the agent's build script for proper dependency management
            if [ -n "${GUEST}" ]; then
                echo "Building ZISK guest programs only..."
                (cd "$ZISK_AGENT_DIR" && ./build.sh guest)
            else
                # Build everything: guest programs + agent service + driver
                echo "Building full ZISK agent system..."
                (cd "$ZISK_AGENT_DIR" && ./build.sh all)
                
                # Build main raiko components with ZISK support
                echo "Building main Raiko with ZISK agent integration..."
                cargo ${TOOLCHAIN_ZISK} build ${FLAGS} --features zisk --package raiko-host --package raiko-pipeline --package raiko-core
            fi
            
        else
            echo "Building ZISK test components..."
            (cd "$ZISK_AGENT_DIR" && ./build.sh guest)
            # Test the agent workspace
            (cd "$ZISK_AGENT_DIR" && cargo test --workspace)
        fi
        
    else
        # RUN mode - can run agent or main raiko
        if [ -z "${TEST}" ]; then
            if [ -n "${ZISK_AGENT}" ]; then
                echo "Starting ZISK agent service..."
                echo "Agent will be available at http://localhost:9998"
                echo "Health check: curl http://localhost:9998/health"
                echo "Press Ctrl+C to stop"
                (cd "$ZISK_AGENT_DIR" && ./target/release/zisk-agent ${ZISK_AGENT_ARGS})
            else
                echo "Running main Raiko with ZISK agent integration..."
                echo "Make sure ZISK agent is running at: \$ZISK_AGENT_URL (default: http://localhost:9998/proof)"
                cargo ${TOOLCHAIN_ZISK} run ${FLAGS} --features zisk
            fi
        else
            echo "Running ZISK integration tests..."
            cargo ${TOOLCHAIN_ZISK} test ${FLAGS} -p raiko-host -p raiko-core --features "zisk,enable"
            (cd "$ZISK_AGENT_DIR" && cargo test --workspace)
        fi
    fi
    
    # Display helpful information
    echo ""
    echo "=== ZISK Agent Information ==="
    echo "Agent directory: $ZISK_AGENT_DIR"
    echo "Build script:    $ZISK_AGENT_DIR/build.sh"
    echo "Agent binary:    $ZISK_AGENT_DIR/target/release/zisk-agent"
    echo "Default port:    9998"
    echo ""
    echo "Usage examples:"
    echo "  # Start agent:        ZISK_AGENT=1 RUN=1 ./script/build.sh zisk"
    echo "  # Run with agent:     RUN=1 ./script/build.sh zisk" 
    echo "  # Build guest only:   GUEST=1 ./script/build.sh zisk"
    echo "  # Agent build script: cd $ZISK_AGENT_DIR && ./build.sh help"
    echo ""
fi
