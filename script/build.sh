#!/usr/bin/env bash

# Any error will result in failure
set -e

# GPU support for Zisk proofs is automatically enabled when CUDA toolkit is detected.
# Manual override: GPU=1 ./script/build.sh zisk (will show warning if CUDA not found)
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

# ZISK
if [ "$1" == "zisk" ]; then
    check_toolchain $TOOLCHAIN_ZISK
    
    # Clear any RISC-V related environment variables that might interfere
    unset CC TARGET_CC
    
    # Check if cargo-zisk is installed
    if ! command -v cargo-zisk &> /dev/null; then
        echo "cargo-zisk not found. Please install Zisk toolchain first:"
        echo "  TARGET=zisk make install"
        echo "or manually:"
        echo "  curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash"
        exit 1
    fi
    
    # Setup feature flags based on GPU availability
    ZISK_COMPUTE_TYPE="CPU"
    ZISK_FEATURES="zisk"
    
    # Check for CUDA toolkit availability
    if command -v nvcc &> /dev/null; then
        ZISK_COMPUTE_TYPE="GPU"
        echo "CUDA toolkit detected: $(nvcc --version | grep 'release' | head -1)"
        echo "   GPU support available for Zisk proof generation"
        echo "   Use cargo-zisk prove with GPU features for accelerated proving"
    elif [ "$GPU" = "1" ]; then
        echo "Warning: GPU=1 specified but CUDA toolkit not found."
        echo "GPU support requires NVIDIA GPU with CUDA toolkit installed."
        echo "Please install CUDA toolkit from: https://developer.nvidia.com/cuda-toolkit"
        echo "Building with CPU support..."
        ZISK_COMPUTE_TYPE="CPU"
    else
        echo "CUDA toolkit not found. Building with CPU support."
        echo "To enable GPU support, install CUDA toolkit: https://developer.nvidia.com/cuda-toolkit"
        ZISK_COMPUTE_TYPE="CPU"
    fi
    
    if [ "$MOCK" = "1" ]; then
        export ZISK_PROVER=mock
        echo "ZISK_PROVER is set to $ZISK_PROVER"
    fi
    
    # Set up RISC-V64 bare-metal cross-compiler for Zisk guest programs
    # Try different compiler locations in order of preference
    if command -v riscv64-unknown-elf-gcc >/dev/null 2>&1; then
        # System-installed bare-metal compiler
        RISCV64_CC="riscv64-unknown-elf-gcc"
        RISCV64_AR="riscv64-unknown-elf-ar"
    elif [ -f /opt/riscv64/bin/riscv64-unknown-elf-gcc ]; then
        # Downloaded bare-metal compiler
        RISCV64_CC="/opt/riscv64/bin/riscv64-unknown-elf-gcc"
        RISCV64_AR="/opt/riscv64/bin/riscv64-unknown-elf-ar"
    elif [ -f /opt/riscv/bin/riscv-none-elf-gcc ] && /opt/riscv/bin/riscv-none-elf-gcc -march=rv64ima -mabi=lp64 -S -o /dev/null -xc /dev/null 2>/dev/null; then
        # Existing compiler that supports 64-bit
        RISCV64_CC="/opt/riscv/bin/riscv-none-elf-gcc"
        RISCV64_AR="/opt/riscv/bin/riscv-none-elf-ar"
    else
        echo "Warning: No suitable RISC-V64 bare-metal compiler found."
        echo "Please run 'TARGET=zisk make install' first."
        RISCV64_CC="riscv64-unknown-elf-gcc"  # Fallback
        RISCV64_AR="riscv64-unknown-elf-ar"
    fi
    
    export CC_riscv64ima_zisk_zkvm_elf="$RISCV64_CC"
    export AR_riscv64ima_zisk_zkvm_elf="$RISCV64_AR"
    export CFLAGS_riscv64ima_zisk_zkvm_elf="-march=rv64ima -mabi=lp64 -ffreestanding -fno-builtin"
    
    if [ -n "${CLIPPY}" ]; then
        cargo ${TOOLCHAIN_ZISK} clippy -p raiko-host -F "${ZISK_FEATURES},enable"
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building Zisk prover with ${ZISK_COMPUTE_TYPE} support"
            echo "Using RISC-V64 bare-metal cross-compiler: $RISCV64_CC"
            cargo ${TOOLCHAIN_ZISK} run --manifest-path provers/zisk/builder/Cargo.toml --bin zisk-builder --no-default-features --features ${ZISK_FEATURES}
            # Set default Zisk image IDs for consistency with other zkVMs
            # Check multiple indicators that we're in a container/CI environment
            # if [ -f "/.dockerenv" ] || [ -n "${DOCKER_BUILDKIT}" ] || [ -n "${CI}" ] || [ ! -f ".env" ] || grep -q docker /proc/1/cgroup 2>/dev/null; then
            #     echo "Container/CI build detected, skipping .env update (will be handled by publish-image.sh)"
            # else
            #     echo "Setting default Zisk image IDs..."
            #     ./script/update_imageid.sh zisk
            # fi
        else
            echo "Building test programs for Zisk prover with ${ZISK_COMPUTE_TYPE} support (features: ${ZISK_FEATURES})"
            cargo ${TOOLCHAIN_ZISK} run --manifest-path provers/zisk/builder/Cargo.toml --bin zisk-builder --no-default-features --features ${ZISK_FEATURES},test,bench
        fi
        if [ -z "${GUEST}" ]; then
            echo "Building Zisk host with ${ZISK_COMPUTE_TYPE} support (features: ${ZISK_FEATURES})"
            # Clear RISC-V CC environment variables for host build
            unset CC TARGET_CC
            cargo ${TOOLCHAIN_ZISK} build ${FLAGS} --no-default-features --features ${ZISK_FEATURES} --package raiko-host --package raiko-pipeline --package raiko-core
        fi
    else
        if [ -z "${TEST}" ]; then
            # Clear RISC-V CC environment variables for host run
            unset CC TARGET_CC
            cargo ${TOOLCHAIN_ZISK} run ${FLAGS} --no-default-features --features ${ZISK_FEATURES}
        else
            cargo ${TOOLCHAIN_ZISK} test ${FLAGS} -p raiko-host -p zisk-driver --no-default-features --features "${ZISK_FEATURES},enable"
        fi
    fi
fi
