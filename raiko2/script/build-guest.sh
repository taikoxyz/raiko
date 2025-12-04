#!/usr/bin/env bash
#
# Raiko V2 Guest Build Script
# Builds zkVM guest programs for RISC0 and SP1 backends.
#
# Usage:
#   ./script/build-guest.sh risc0 [--bench]
#   ./script/build-guest.sh sp1 [--bench]
#   ./script/build-guest.sh all [--bench]
#
# Environment:
#   PROFILE   - Build profile (release/debug, default: release)
#   MOCK      - If set to 1, enables mock/dev mode
#   VERBOSE   - If set to 1, enables verbose output
#

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PROFILE="${PROFILE:-release}"
VERBOSE="${VERBOSE:-0}"

# Toolchain versions
TOOLCHAIN_RISC0="+nightly-2024-12-20"
TOOLCHAIN_SP1="+nightly-2024-12-20"

# Guest directories (V2 locations)
RISC0_GUEST_DIR="$ROOT_DIR/crates/guest-risc0"
SP1_GUEST_DIR="$ROOT_DIR/crates/guest-sp1"

# Output directories
RISC0_OUTPUT_DIR="$ROOT_DIR/crates/prover/src/risc0/methods"
SP1_OUTPUT_DIR="$ROOT_DIR/crates/prover/src/sp1/elf"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_toolchain() {
    local toolchain="${1#+}"  # Remove leading +
    if rustup toolchain list | grep -q "$toolchain"; then
        log_info "Toolchain $toolchain is installed"
    else
        log_info "Installing toolchain $toolchain..."
        rustup install "$toolchain"
    fi
}

# RISC0 guest build
build_risc0() {
    local bench_mode="${1:-false}"
    log_info "Building RISC0 guest programs..."

    check_toolchain "$TOOLCHAIN_RISC0"

    cd "$RISC0_GUEST_DIR"

    # RISC0-specific environment and flags
    export CARGO_TARGET_RISCV32IM_RISC0_ZKVM_ELF_RUSTFLAGS="-C passes=lower-atomic -C link-arg=-Ttext=0x00200800 -C link-arg=--fatal-warnings -C panic=abort --cfg getrandom_backend=\"custom\""
    export CC="/opt/riscv/bin/riscv32-unknown-elf-gcc"
    export CFLAGS="-march=rv32im -mstrict-align -falign-functions=2"
    export RISC0_FEATURE_bigint2=1

    if [ "${MOCK:-0}" = "1" ]; then
        export RISC0_DEV_MODE=1
        log_info "RISC0_DEV_MODE enabled"
    fi

    local profile_flag=""
    if [ "$PROFILE" = "release" ]; then
        profile_flag="--release"
    fi

    # Build Shasta-only binaries
    local bins=("risc0-batch" "risc0-shasta-aggregation")
    if [ "$bench_mode" = "true" ]; then
        bins+=("sha256" "ecdsa")
    fi

    for bin in "${bins[@]}"; do
        log_info "Building $bin..."
        cargo $TOOLCHAIN_RISC0 build \
            --target riscv32im-risc0-zkvm-elf \
            --bin "$bin" \
            $profile_flag \
            --ignore-rust-version \
            ${VERBOSE:+"-v"}
    done

    # Export ELFs to methods directory
    mkdir -p "$RISC0_OUTPUT_DIR"
    local target_dir="$RISC0_GUEST_DIR/target/riscv32im-risc0-zkvm-elf/$PROFILE"

    for bin in "${bins[@]}"; do
        local elf="$target_dir/$bin"
        if [ -f "$elf" ]; then
            # Generate image ID and copy ELF
            local elf_name="${bin//-/_}"
            cp "$elf" "$RISC0_OUTPUT_DIR/${elf_name}.elf"
            log_info "Exported $elf_name.elf"

            # Generate methods.rs with image ID
            # Note: The actual image ID computation would require risc0-zkvm tooling
            # For now, we just copy the ELF and the driver will compute IDs at runtime
        fi
    done

    log_info "RISC0 guest build complete"
}

# SP1 guest build
build_sp1() {
    local bench_mode="${1:-false}"
    log_info "Building SP1 guest programs..."

    check_toolchain "$TOOLCHAIN_SP1"

    cd "$SP1_GUEST_DIR"

    # SP1-specific environment and flags
    export CARGO_TARGET_RISCV32IM_SUCCINCT_ZKVM_ELF_RUSTFLAGS="-C passes=lower-atomic -C link-arg=-Ttext=0x00200800 -C panic=abort"
    export CC="/opt/riscv/bin/riscv32-unknown-elf-gcc"
    export CFLAGS="-march=rv32im -mstrict-align -falign-functions=2"

    if [ "${MOCK:-0}" = "1" ]; then
        export SP1_PROVER=mock
        log_info "SP1_PROVER=mock enabled"
    fi

    local profile_flag=""
    if [ "$PROFILE" = "release" ]; then
        profile_flag="--release"
    fi

    # Build Shasta-only binaries
    local bins=("sp1-batch" "sp1-shasta-aggregation")
    if [ "$bench_mode" = "true" ]; then
        bins+=("sha256" "ecdsa" "bn254_add" "bn254_mul")
    fi

    for bin in "${bins[@]}"; do
        log_info "Building $bin..."
        cargo $TOOLCHAIN_SP1 build \
            --target riscv32im-succinct-zkvm-elf \
            --bin "$bin" \
            $profile_flag \
            --ignore-rust-version \
            ${VERBOSE:+"-v"}
    done

    # Export ELFs to output directory
    mkdir -p "$SP1_OUTPUT_DIR"
    local target_dir="$SP1_GUEST_DIR/target/riscv32im-succinct-zkvm-elf/$PROFILE"

    for bin in "${bins[@]}"; do
        local elf="$target_dir/$bin"
        if [ -f "$elf" ]; then
            local elf_name="${bin//-/_}"
            cp "$elf" "$SP1_OUTPUT_DIR/${elf_name}.elf"
            log_info "Exported $elf_name.elf"
        fi
    done

    log_info "SP1 guest build complete"
}

# Update image IDs in .env
update_image_ids() {
    local backend="$1"
    log_info "Updating image IDs for $backend..."

    if [ -f "$ROOT_DIR/.env" ]; then
        # This would be implemented by calling the existing update_imageid.sh
        # or by computing IDs directly
        "$SCRIPT_DIR/update_imageid.sh" "$backend" || true
    else
        log_warn "No .env file found, skipping image ID update"
    fi
}

print_usage() {
    echo "Usage: $0 <backend> [options]"
    echo ""
    echo "Backends:"
    echo "  risc0   Build RISC0 guest programs"
    echo "  sp1     Build SP1 guest programs"
    echo "  all     Build all guest programs"
    echo ""
    echo "Options:"
    echo "  --bench    Include benchmark binaries"
    echo "  --help     Show this help message"
    echo ""
    echo "Environment variables:"
    echo "  PROFILE=release|debug   Build profile (default: release)"
    echo "  MOCK=1                  Enable mock/dev mode"
    echo "  VERBOSE=1               Enable verbose output"
}

main() {
    if [ $# -lt 1 ]; then
        print_usage
        exit 1
    fi

    local backend="$1"
    shift

    local bench_mode="false"
    while [ $# -gt 0 ]; do
        case "$1" in
            --bench)
                bench_mode="true"
                ;;
            --help)
                print_usage
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                print_usage
                exit 1
                ;;
        esac
        shift
    done

    case "$backend" in
        risc0)
            build_risc0 "$bench_mode"
            update_image_ids risc0
            ;;
        sp1)
            build_sp1 "$bench_mode"
            update_image_ids sp1
            ;;
        all)
            build_risc0 "$bench_mode"
            update_image_ids risc0
            build_sp1 "$bench_mode"
            update_image_ids sp1
            ;;
        --help)
            print_usage
            exit 0
            ;;
        *)
            log_error "Unknown backend: $backend"
            print_usage
            exit 1
            ;;
    esac

    log_info "Build complete!"
}

main "$@"
