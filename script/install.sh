#!/usr/bin/env bash
set -e

# ─── Configuration ─────────────────────────────────────────────────────────────
# ZisK version to install (override via env: ZISK_VERSION=0.16.0)
ZISK_VERSION="${ZISK_VERSION:-0.16.0}"
# ZisK install path (override via env: ZISK_DIR=/ephemeral/.zisk).
# If different from ~/.zisk, a symlink ~/.zisk -> ZISK_DIR is created automatically.
ZISK_DIR="${ZISK_DIR:-$HOME/.zisk}"

# ─── CI check ──────────────────────────────────────────────────────────────────
if [ -n "$CI" ]; then
    source ./script/ci-env-check.sh
fi

# ═══════════════════════════════════════════════════════════════════════════════
# Helper functions
# ═══════════════════════════════════════════════════════════════════════════════

# ─── SP1 ───────────────────────────────────────────────────────────────────────
install_sp1() {
    curl -L https://sp1.succinct.xyz | bash
    echo "SP1 installed"
    source "$HOME/.profile" 2>/dev/null || true
    if command -v sp1up >/dev/null 2>&1; then
        sp1up --c-toolchain
    else
        "$HOME/.sp1/bin/sp1up" --c-toolchain
    fi
}

# ─── ZisK ──────────────────────────────────────────────────────────────────────

# Create ZISK_DIR and, when using a custom path, symlink ~/.zisk -> ZISK_DIR.
# Must be called before any ziskup/cargo-zisk commands.
setup_zisk_dir() {
    mkdir -p "$ZISK_DIR"
    if [ "$ZISK_DIR" != "$HOME/.zisk" ]; then
        if [ -e "$HOME/.zisk" ] && [ ! -L "$HOME/.zisk" ]; then
            echo "Error: $HOME/.zisk exists and is not a symlink."
            echo "Remove it manually before using a custom ZISK_DIR."
            exit 1
        fi
        ln -sfn "$ZISK_DIR" "$HOME/.zisk"
        echo "Symlinked $HOME/.zisk -> $ZISK_DIR"
    fi
}

# Run ziskup, preferring the already-installed binary; falls back to the install script.
run_ziskup() {
    if [ -x "$ZISK_DIR/bin/ziskup" ]; then
        ZISK_DIR="$ZISK_DIR" "$ZISK_DIR/bin/ziskup" "$@"
    else
        ZISK_DIR="$ZISK_DIR" curl -s \
            https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh \
            | bash -s -- "$@"
    fi
}

install_zisk_cli() {
    if command -v cargo-zisk >/dev/null 2>&1; then
        echo "Zisk already installed: $(cargo-zisk --version)"
        return 0
    fi
    echo "Installing Zisk v$ZISK_VERSION..."
    # The upstream ziskup install script automatically runs ziskup after
    # installing the binary, which prompts for key options interactively.
    # In non-TTY environments (Docker), pipe "4" (None) to skip the menu.
    # When --nokey is passed, ziskup may still prompt if the flag isn't
    # forwarded to the inner invocation.
    echo "4" | run_ziskup --version "$ZISK_VERSION" --nokey || true
    export PATH="$ZISK_DIR/bin:$PATH"
    source "$HOME/.profile" 2>/dev/null || source "$HOME/.bashrc" 2>/dev/null || true
    # If the piped install didn't fully complete, run ziskup directly with --nokey
    if ! command -v cargo-zisk >/dev/null 2>&1; then
        if [ -x "$ZISK_DIR/bin/ziskup" ]; then
            echo "Running ziskup directly with --nokey..."
            "$ZISK_DIR/bin/ziskup" --version "$ZISK_VERSION" --nokey
            export PATH="$ZISK_DIR/bin:$PATH"
        fi
    fi
    command -v cargo-zisk >/dev/null 2>&1 || {
        echo "Error: Failed to install Zisk. Install manually:"
        echo "  curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash"
        exit 1
    }
}

install_zisk_toolchain() {
    echo "Installing Zisk Rust toolchain..."
    if cargo-zisk sdk install-toolchain; then
        return 0
    fi
    echo "Automatic toolchain installation failed"
	exit 1
}

ensure_zisk_proving_keys() {
    if [ ! -d "$ZISK_DIR/provingKey" ]; then
        echo "Installing Zisk proving key..."
        run_ziskup --version "$ZISK_VERSION" --provingkey
    else
        echo "Zisk proving key already present"
    fi

    if [ ! -d "$ZISK_DIR/provingKeySnark" ]; then
        echo "Installing Zisk SNARK proving key..."
        run_ziskup setup_snark
    else
        echo "Zisk SNARK proving key already present"
    fi
}

run_zisk_check_setup() {
    echo "Regenerating Zisk constant tree files..."
    "$ZISK_DIR/bin/cargo-zisk" check-setup -a --snark 
}

copy_zisk_gpu_binaries() {
    local src="$1"

    mkdir -p "$ZISK_DIR/bin"
    cp "$src/cargo-zisk" "$src/ziskemu" "$src/riscv2zisk" \
       "$src/zisk-coordinator" "$src/zisk-worker" "$src/libziskclib.a" \
       "$ZISK_DIR/bin/"

    mkdir -p "$ZISK_DIR/zisk/emulator-asm"
    cp -r ./emulator-asm/src "$ZISK_DIR/zisk/emulator-asm/"
    cp ./emulator-asm/Makefile "$ZISK_DIR/zisk/emulator-asm/"
    cp -r ./lib-c "$ZISK_DIR/zisk/"
}

build_zisk_gpu() {
    local marker="$ZISK_DIR/.gpu-enabled"
    local built_version
    built_version=$(cat "$marker" 2>/dev/null || echo "")

    if [ "$built_version" = "$ZISK_VERSION" ]; then
        echo "Zisk GPU support already built for v$ZISK_VERSION"
        return 0
    fi

    if [ -n "$built_version" ]; then
        echo "Zisk GPU version mismatch (built: $built_version, wanted: $ZISK_VERSION), rebuilding..."
    fi

    echo "Building Zisk with GPU support (tag v$ZISK_VERSION)..."
    local tmp
    tmp=$(mktemp -d)
    (
        git clone --depth=1 --branch "v$ZISK_VERSION" \
            https://github.com/0xPolygonHermez/zisk.git "$tmp/zisk"
        cd "$tmp/zisk"
        if cargo build --release --features gpu; then
            copy_zisk_gpu_binaries "target/release"
            echo "$ZISK_VERSION" > "$marker"
            echo "Zisk successfully built with GPU support!"
            run_zisk_check_setup
        else
            echo "GPU build failed, continuing with existing binaries"
        fi
    )
    rm -rf "$tmp"
}

# ═══════════════════════════════════════════════════════════════════════════════
# Installation sections
# ═══════════════════════════════════════════════════════════════════════════════

# ─── RISC-V64 bare-metal toolchain (needed by ZisK guest) ──────────────────────
# if [ -z "$1" ] || [ "$1" == "zisk" ]; then
#     if [ -f /opt/riscv/bin/riscv-none-elf-gcc ]; then
#         echo "Checking existing RISC-V toolchain for 64-bit support..."
#         if /opt/riscv/bin/riscv-none-elf-gcc -march=rv64ima -mabi=lp64 -S -o /dev/null -xc /dev/null 2>/dev/null; then
#             echo "Existing RISC-V toolchain supports 64-bit"
#         else
#             echo "Warning: Existing RISC-V toolchain doesn't support 64-bit."
#         fi
#     else
#         echo "Installing bare-metal RISC-V64 cross-compiler toolchain..."
#         if command -v apt-get >/dev/null 2>&1; then
#             sudo apt-get update
#             if ! sudo apt-get install -y gcc-riscv64-unknown-elf 2>/dev/null; then
#                 echo "gcc-riscv64-unknown-elf not available, downloading prebuilt toolchain..."
#                 local riscv_archive="/tmp/riscv64-unknown-elf-gcc.tar.gz"
#                 wget -O "$riscv_archive" \
#                     "https://github.com/riscv-collab/riscv-gnu-toolchain/releases/download/2024.02.02/riscv64-elf-ubuntu-22.04-gcc-nightly-2024.02.02-nightly.tar.gz" \
#                     && sudo mkdir -p /opt/riscv64 \
#                     && sudo tar -xzf "$riscv_archive" -C /opt/riscv64 --strip-components=1 \
#                     || echo "Warning: Could not install RISC-V64 toolchain. Please install manually."
#             fi
#         else
#             echo "Warning: Could not install RISC-V64 toolchain automatically (no apt-get)."
#         fi
#     fi
# fi

# ─── SGX ───────────────────────────────────────────────────────────────────────
if [ -z "$1" ] || [ "$1" == "sgx" ]; then
    if command -v gramine-sgx >/dev/null 2>&1; then
        echo "gramine already installed"
    else
        echo "Installing gramine..."
        sudo curl -fsSLo /etc/apt/keyrings/gramine-keyring-$(lsb_release -sc).gpg \
            https://packages.gramineproject.io/gramine-keyring-$(lsb_release -sc).gpg
        echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/gramine-keyring-$(lsb_release -sc).gpg] \
https://packages.gramineproject.io/ $(lsb_release -sc) main" \
            | sudo tee /etc/apt/sources.list.d/gramine.list
        sudo curl -fsSLo /etc/apt/keyrings/intel-sgx-deb.asc \
            https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key
        echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/intel-sgx-deb.asc] \
https://download.01.org/intel-sgx/sgx_repo/ubuntu $(lsb_release -sc) main" \
            | sudo tee /etc/apt/sources.list.d/intel-sgx.list
        sudo apt-get update
        sudo apt-get install -y gramine
        echo "Gramine installed. REBOOT MAY BE REQUIRED."
    fi
fi

# ─── RISC0 ─────────────────────────────────────────────────────────────────────
if [ -z "$1" ] || [ "$1" == "risc0" ]; then
    if [ -z "$TERM" ] || [ "$TERM" = "dumb" ]; then
        export TERM=xterm
    fi
    curl -L https://risczero.com/install | bash

    env_rzup=rzup
    if [ -z "${CI}" ] || ! command -v rzup >/dev/null 2>&1; then
        source "$HOME/.bashrc" 2>/dev/null || true
        if ! command -v rzup >/dev/null 2>&1; then
            export PATH="$HOME/.risc0/bin:$PATH"
            env_rzup="$HOME/.risc0/bin/rzup"
        fi
    else
        echo "/home/runner/.risc0/bin" >> "$GITHUB_PATH"
        echo "/home/runner/.config/.risc0/bin" >> "$GITHUB_PATH"
        env_rzup=/home/runner/.risc0/bin/rzup
    fi

    command -v "$env_rzup" >/dev/null 2>&1 || { echo "Error: rzup not found; please reinstall."; exit 1; }
    $env_rzup install
    $env_rzup install risc0-groth16
fi

# ─── SP1 ───────────────────────────────────────────────────────────────────────
if [ -z "$1" ] || [ "$1" == "sp1" ]; then
    install_sp1
fi

# ─── ZisK ──────────────────────────────────────────────────────────────────────
if [ -z "$1" ] || [ "$1" == "zisk" ]; then
    setup_zisk_dir
    install_sp1
    install_zisk_cli
    install_zisk_toolchain

    # Install proving keys unless explicitly disabled (INSTALL_KEYS=false)
    if [ "${INSTALL_KEYS:-true}" != "false" ]; then
        ensure_zisk_proving_keys
    else
        echo "Skipping Zisk proving key installation (INSTALL_KEYS=false)"
    fi

    if command -v nvcc >/dev/null 2>&1; then
        echo "CUDA toolkit detected, building Zisk with GPU support..."
        build_zisk_gpu
    fi
fi

# ─── TDX ───────────────────────────────────────────────────────────────────────
if [ -z "$1" ] || [ "$1" == "tdx" ]; then
    echo "TDX prover doesn't require additional toolchain installation"
fi
