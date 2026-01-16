#!/usr/bin/env bash

# Any error will result in failure
set -e

TOOLCHAIN_RISC0=+nightly-2024-12-20
TOOLCHAIN_SP1=+nightly-2024-12-20
TOOLCHAIN_SGX=+nightly-2024-12-20
TOOLCHAIN_BREVIS=+nightly-2025-08-04


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
            # The legacy .env update step called an unsupported 'sgx' mode in update_imageid.sh;
            # disable it for now so TARGET=sgx builds can complete without an error.
            # # Check multiple indicators that we're in a container/CI environment
            # if [ -f "/.dockerenv" ] || [ -n "${DOCKER_BUILDKIT}" ] || [ -n "${CI}" ] || [ ! -f ".env" ] || grep -q docker /proc/1/cgroup 2>/dev/null; then
            #     echo "Container/CI build detected, skipping MRENCLAVE .env update (will be handled by publish-image.sh)"
            # else
            #     ./script/update_imageid.sh sgx
            # fi
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
        cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder
        cargo ${TOOLCHAIN_RISC0} clippy -F risc0
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
            echo "Building Risc0 prover"
            cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder 2>&1 | tee /tmp/risc0_build_output.txt
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

# BREVIS PICO
if [ "$1" == "brevis" ]; then
    check_toolchain $TOOLCHAIN_BREVIS
    rustup component add rust-src --toolchain "${TOOLCHAIN_BREVIS#+}"
    if [ -z "${RISCV_GCC_DIR}" ]; then
        if [ -x "/opt/riscv/bin/riscv32-unknown-elf-gcc" ]; then
            RISCV_GCC_DIR="/opt/riscv"
        elif [ -x "${HOME}/.riscv/bin/riscv32-unknown-elf-gcc" ]; then
            RISCV_GCC_DIR="${HOME}/.riscv"
        fi
    fi
    if [ -n "${RISCV_GCC_DIR}" ] && [ -x "${RISCV_GCC_DIR}/bin/riscv32-unknown-elf-gcc" ]; then
        if [ -z "${TARGET_CC}" ]; then
            export TARGET_CC="${RISCV_GCC_DIR}/bin/riscv32-unknown-elf-gcc"
        fi
        if [ -z "${CC_riscv32im_risc0_zkvm_elf}" ]; then
            export CC_riscv32im_risc0_zkvm_elf="${RISCV_GCC_DIR}/bin/riscv32-unknown-elf-gcc"
        fi
        if [ -z "${AR_riscv32im_risc0_zkvm_elf}" ]; then
            export AR_riscv32im_risc0_zkvm_elf="${RISCV_GCC_DIR}/bin/riscv32-unknown-elf-ar"
        fi
    elif command -v riscv32-unknown-elf-gcc >/dev/null 2>&1; then
        if [ -z "${TARGET_CC}" ]; then
            export TARGET_CC="$(command -v riscv32-unknown-elf-gcc)"
        fi
        if [ -z "${CC_riscv32im_risc0_zkvm_elf}" ]; then
            export CC_riscv32im_risc0_zkvm_elf="$(command -v riscv32-unknown-elf-gcc)"
        fi
        if [ -z "${AR_riscv32im_risc0_zkvm_elf}" ] && command -v riscv32-unknown-elf-ar >/dev/null 2>&1; then
            export AR_riscv32im_risc0_zkvm_elf="$(command -v riscv32-unknown-elf-ar)"
        fi
    fi

    if ! command -v cargo-pico >/dev/null 2>&1; then
        echo "cargo-pico not found. Run ./script/install.sh brevis first."
        exit 1
    fi

    BREVIS_GUEST_DIR="./provers/brevis/guest"
    BREVIS_ELF_DIR="${BREVIS_GUEST_DIR}/elf"
    BREVIS_BATCH_ELF_DEFAULT="${BREVIS_ELF_DIR}/brevis-batch"
    BREVIS_AGG_ELF_DEFAULT="${BREVIS_ELF_DIR}/brevis-aggregation"
    BREVIS_SHASTA_AGG_ELF_DEFAULT="${BREVIS_ELF_DIR}/brevis-shasta-aggregation"

    if [ -n "${CLIPPY}" ]; then
        cargo ${TOOLCHAIN_BREVIS} clippy -p raiko-host -p brevis-driver --features "brevis,enable" -- -D warnings
    elif [ -z "${RUN}" ]; then
        if [ -z "${TEST}" ]; then
                echo "Building Brevis guest ELFs"
            BREVIS_CARGO_ENCODED_RUSTFLAGS=$(
                printf "%s\x1f%s\x1f%s\x1f%s\x1f%s\x1f%s\x1f%s\x1f%s" \
                    "-C" "passes=lower-atomic" \
                    "-C" "link-arg=-Ttext=0x00200800" \
                    "-C" "link-arg=--fatal-warnings" \
                    "-C" "panic=abort"
            )

            build_brevis_elf() {
                local BIN_NAME="$1"

                (cd "${BREVIS_GUEST_DIR}" && CARGO_ENCODED_RUSTFLAGS="${BREVIS_CARGO_ENCODED_RUSTFLAGS}" \
                    cargo ${TOOLCHAIN_BREVIS} build --release \
                        --bin "${BIN_NAME}" \
                        --target riscv32im-risc0-zkvm-elf \
                        -Z build-std=alloc,core,proc_macro,panic_abort,std \
                        -Z build-std-features=compiler-builtins-mem \
                        --target-dir "${BREVIS_GUEST_DIR}/target")

                mkdir -p "${BREVIS_ELF_DIR}"
                cp "${BREVIS_GUEST_DIR}/target/riscv32im-risc0-zkvm-elf/release/${BIN_NAME}" "${BREVIS_ELF_DIR}/${BIN_NAME}"
            }

            build_brevis_elf "brevis-batch"
            build_brevis_elf "brevis-aggregation"
            build_brevis_elf "brevis-shasta-aggregation"

            # Keep .env in sync with the latest Brevis VKEYs when building locally.
            if [ -f "/.dockerenv" ] || [ -n "${DOCKER_BUILDKIT}" ] || [ -n "${CI}" ] || [ ! -f ".env" ] || grep -q docker /proc/1/cgroup 2>/dev/null; then
                echo "Container/CI build detected, skipping Brevis VKEY .env update (will be handled by publish-image.sh)"
            else
                echo "Updating environment with new Brevis VKEYs..."
                ./script/update_imageid.sh brevis "${BREVIS_ELF_DIR}"
            fi

            if [ -z "${GUEST}" ]; then
                echo "Building Brevis prover host"
                cargo ${TOOLCHAIN_BREVIS} build ${FLAGS} --features brevis
            fi
        else
            echo "Building Brevis tests"
            cargo ${TOOLCHAIN_BREVIS} test ${FLAGS} --features brevis --no-run
        fi
    else
        if [ -z "${TEST}" ]; then
            echo "Running Brevis prover"
            : "${BREVIS_BATCH_ELF:=${BREVIS_BATCH_ELF_DEFAULT}}"
            : "${BREVIS_AGG_ELF:=${BREVIS_AGG_ELF_DEFAULT}}"
            : "${BREVIS_SHASTA_AGG_ELF:=${BREVIS_SHASTA_AGG_ELF_DEFAULT}}"
            export BREVIS_BATCH_ELF BREVIS_AGG_ELF BREVIS_SHASTA_AGG_ELF
            cargo ${TOOLCHAIN_BREVIS} run ${FLAGS} --features brevis
        else
            echo "Running Brevis tests"
            cargo ${TOOLCHAIN_BREVIS} test ${FLAGS} --features brevis
        fi
    fi
fi
