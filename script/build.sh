#!/usr/bin/env bash

# Any error will result in failure
set -e

TOOLCHAIN_RISC0=+nightly-2024-12-20
TOOLCHAIN_SP1=+nightly-2024-12-20
TOOLCHAIN_SGX=+nightly-2024-12-20

# Store original CC environment variable to restore later if needed
ORIGINAL_CC="${CC:-}"

# Function to set up CC environment for different targets
setup_cc_env() {
    local TARGET=$1
    case "$TARGET" in
        "native")
            # Use system default compiler for native builds
            if [ -n "$ORIGINAL_CC" ] && [ "$ORIGINAL_CC" != "riscv64-unknown-elf-gcc" ]; then
                export CC="$ORIGINAL_CC"
            else
                # Force unset CC to use system default (gcc/clang)
                unset CC
            fi
            echo "CC environment for native: ${CC:-system default}"
            ;;
        "risc0")
            # RISC0 requires specific compiler setup
            export CC="clang"
            echo "CC environment for risc0: $CC"
            ;;
        "sp1")
            # SP1 requires specific compiler setup
            export CC="clang"
            echo "CC environment for sp1: $CC"
            ;;
        "sgx")
            # SGX uses system default
            if [ -n "$ORIGINAL_CC" ] && [ "$ORIGINAL_CC" != "riscv64-unknown-elf-gcc" ]; then
                export CC="$ORIGINAL_CC"
            else
                # Force unset CC to use system default (gcc/clang)
                unset CC
            fi
            echo "CC environment for sgx: ${CC:-system default}"
            ;;
        *)
            echo "Unknown target for CC setup: $TARGET"
            ;;
    esac
}

check_toolchain() {
    local TOOLCHAIN=$1

    # Remove the plus sign from the toolchain name
    TOOLCHAIN=${TOOLCHAIN#+}

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
    setup_cc_env "native"
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
    setup_cc_env "sgx"
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
    setup_cc_env "risc0"
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
        cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder
        cargo ${TOOLCHAIN_RISC0} clippy -F risc0
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building Risc0 prover"
            cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder 2>&1 | tee /tmp/risc0_build_output.txt
            echo "Updating environment with new RISC0 image IDs..."
            ./script/update_imageid.sh risc0
        else
            echo "Building test elfs for Risc0 prover"
            cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder --features test,bench
        fi
        if [ -z "${GUEST}" ]; then
            cargo ${TOOLCHAIN_RISC0} build ${FLAGS} --features risc0
        fi
    else
        if [ -z "${TEST}" ]; then
            echo "Running Risc0 prover"
            cargo ${TOOLCHAIN_RISC0} run ${FLAGS} --features risc0
        else
            echo "Running Risc0 tests"
            cargo ${TOOLCHAIN_RISC0} test ${FLAGS} --lib risc0-driver --features risc0  -- run_unittest_elf
            cargo ${TOOLCHAIN_RISC0} test ${FLAGS} -p raiko-host -p risc0-driver --features "risc0 enable"
        fi
    fi
fi

# SP1
if [ "$1" == "sp1" ]; then
    setup_cc_env "sp1"
    check_toolchain $TOOLCHAIN_SP1
    if [ "$MOCK" = "1" ]; then
        export SP1_PROVER=mock
        echo "SP1_PROVER is set to $SP1_PROVER"
    fi
    if [ -n "${CLIPPY}" ]; then
        cargo ${TOOLCHAIN_SP1} clippy -p raiko-host -p sp1-builder -p sp1-driver -F "sp1,enable"
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building Sp1 prover"
            cargo ${TOOLCHAIN_SP1} run --bin sp1-builder 2>&1 | tee /tmp/sp1_build_output.txt
            echo "Updating environment with new SP1 VK hashes..."
            ./script/update_imageid.sh sp1
        else
            echo "Building test elfs for Sp1 prover"
            cargo ${TOOLCHAIN_SP1} run --bin sp1-builder --features test,bench
        fi
        if [ -z "${GUEST}" ]; then
            echo "Building 'cargo ${TOOLCHAIN_SP1} build ${FLAGS} --features sp1'" 
            cargo ${TOOLCHAIN_SP1} build ${FLAGS} --features sp1
        fi
    else
        if [ -z "${TEST}" ]; then
            echo "Running Sp1 prover"
            cargo ${TOOLCHAIN_SP1} run ${FLAGS} --features sp1
        else
            echo "Running Sp1 unit tests"
            # cargo ${TOOLCHAIN_SP1} test ${FLAGS} --lib sp1-driver --features sp1 -- run_unittest_elf
            cargo ${TOOLCHAIN_SP1} test ${FLAGS} -p raiko-host -p sp1-driver --features "sp1 enable"

            # Don't want to span Succinct Network and wait 2 hours in CI
            # echo "Running Sp1 verification"
            # cargo ${TOOLCHAIN_SP1} run ${FLAGS} --bin sp1-verifier --features enable,sp1-verifier
        fi
    fi
fi
