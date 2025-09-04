#!/bin/bash
set -e

# ZISK Agent Consolidated Build Script
# This script handles building all ZISK components in the consolidated structure

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RAIKO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log() {
    echo -e "${GREEN}[ZISK Agent]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[ZISK Agent]${NC} $1"
}

error() {
    echo -e "${RED}[ZISK Agent]${NC} $1"
}

# Check if cargo-zisk is installed
check_zisk_toolchain() {
    if ! command -v cargo-zisk &> /dev/null; then
        error "cargo-zisk not found. Please install ZISK toolchain:"
        echo "  curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash"
        echo "  source ~/.bashrc"
        exit 1
    fi
    
    log "Found cargo-zisk: $(which cargo-zisk)"
    
    # Check if ZISK target is available
    if ! rustc --print target-list | grep -q "riscv64ima-zisk-zkvm-elf"; then
        warn "ZISK target 'riscv64ima-zisk-zkvm-elf' not found in rustc"
        warn "This is expected - ZISK uses its own custom target"
        log "cargo-zisk will handle the custom target compilation"
    fi
}

# Build ZISK guest programs (batch and aggregation)
build_guest_programs() {
    log "Building ZISK guest programs..."
    
    # Navigate to guest directory (now inside agent)
    cd "$SCRIPT_DIR/guest"
    
    # Check if guest directory exists
    if [ ! -f "Cargo.toml" ]; then
        error "ZISK guest Cargo.toml not found at $(pwd)"
        exit 1
    fi
    
    # Set CC to clang for ZISK compilation and clear RISC-V related environment variables
    export CC=clang
    unset TARGET_CC
    
    # Build guest programs using cargo-zisk (not regular cargo)
    log "Building with cargo-zisk for riscv64ima-zisk-zkvm-elf target..."
    
    # Use cargo-zisk for RISC-V compilation, not regular cargo
    # cargo-zisk handles the custom target automatically
    log "Running: cargo-zisk build --release"
    cargo-zisk build --release
    
    # Create ELF directory in guest if it doesn't exist
    mkdir -p "$SCRIPT_DIR/guest/elf"
    
    # Copy ELF files to guest/elf directory
    # cargo-zisk might use a different output structure, check both possibilities
    ELF_SOURCE_DIR="target/riscv64ima-zisk-zkvm-elf/release"
    FALLBACK_ELF_DIR="target/release"
    
    # Function to find and copy ELF file
    copy_elf() {
        local elf_name="$1"
        local found=false
        
        for search_dir in "$ELF_SOURCE_DIR" "$FALLBACK_ELF_DIR"; do
            if [ -f "$search_dir/$elf_name" ]; then
                cp "$search_dir/$elf_name" "$SCRIPT_DIR/guest/elf/"
                log "Copied $elf_name ELF from $search_dir/ to guest/elf/"
                found=true
                break
            fi
        done
        
        if [ "$found" = false ]; then
            error "$elf_name ELF not found in $ELF_SOURCE_DIR or $FALLBACK_ELF_DIR"
            log "Available files in target directories:"
            find target -name "$elf_name" 2>/dev/null || echo "  No $elf_name files found"
            return 1
        fi
    }
    
    copy_elf "zisk-batch"
    copy_elf "zisk-aggregation"
    
    log "Guest programs built successfully"
}

# Build the agent service
build_agent() {
    log "Building ZISK agent service..."
    
    cd "$SCRIPT_DIR"
    
    # Set CC to clang for ZISK compilation and clear RISC-V related environment variables
    export CC=clang
    unset TARGET_CC
    
    # Build the agent binary (now in workspace)
    cargo build --release -p zisk-agent-service
    
    if [ -f "target/release/zisk-agent" ]; then
        log "Agent service built successfully: target/release/zisk-agent"
    else
        error "Failed to build agent service"
        exit 1
    fi
}

# Build the driver
build_driver() {
    log "Building ZISK agent driver..."
    
    cd "$SCRIPT_DIR"
    
    # Set CC to clang for ZISK compilation and clear RISC-V related environment variables
    export CC=clang
    unset TARGET_CC
    
    # Build the driver
    cargo build --release -p zisk-agent-driver
    
    log "Driver built successfully"
}

# Clean build artifacts
clean() {
    log "Cleaning build artifacts..."
    
    # Clean guest build
    cd "$SCRIPT_DIR/guest"
    cargo clean
    
    # Clean workspace builds
    cd "$SCRIPT_DIR"
    cargo clean
    
    # Remove ELF files
    rm -rf "$SCRIPT_DIR/guest/elf"
    
    log "Clean completed"
}

# Check CUDA availability for GPU support
check_gpu_support() {
    if command -v nvcc &> /dev/null; then
        log "CUDA toolkit found: $(nvcc --version | head -1)"
        export ZISK_GPU_SUPPORT=1
    else
        warn "CUDA toolkit not found - GPU acceleration disabled"
        export ZISK_GPU_SUPPORT=0
    fi
}

# Display help
show_help() {
    echo "ZISK Agent Build Script"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  guest     Build only guest programs (ELF files)"
    echo "  agent     Build only agent service"
    echo "  driver    Build only driver component"
    echo "  workspace Build workspace components (agent + driver)"
    echo "  all       Build everything (guest + workspace) (default)"
    echo "  clean     Clean build artifacts"
    echo "  check     Check toolchain and dependencies"
    echo "  help      Show this help message"
    echo ""
    echo "Environment Variables:"
    echo "  CARGO_TARGET_DIR    Override cargo target directory"
    echo "  RUST_LOG           Set logging level (default: info)"
    echo ""
}

# Check dependencies and environment
check_dependencies() {
    log "Checking dependencies..."
    
    # Check Rust toolchain
    if ! command -v cargo &> /dev/null; then
        error "Rust/Cargo not found"
        exit 1
    fi
    
    # Check nightly toolchain
    if ! rustup toolchain list | grep -q "nightly-2024-12-20"; then
        warn "Required nightly toolchain not found, installing..."
        rustup toolchain install nightly-2024-12-20
    fi
    
    check_zisk_toolchain
    check_gpu_support
    
    log "All dependencies satisfied"
}

# Main script logic
main() {
    case "${1:-all}" in
        guest)
            check_dependencies
            build_guest_programs
            ;;
        agent)
            build_agent
            ;;
        driver)
            build_driver
            ;;
        workspace)
            build_agent
            build_driver
            ;;
        all)
            check_dependencies
            build_guest_programs
            build_agent
            build_driver
            ;;
        clean)
            clean
            ;;
        check)
            check_dependencies
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            error "Unknown command: $1"
            show_help
            exit 1
            ;;
    esac
}

# Run main function
main "$@"