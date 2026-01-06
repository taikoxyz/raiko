# ZISK Agent Assets (Service Deprecated)

This directory keeps the ZISK guest programs and the Raiko driver. The standalone
`zisk-agent` HTTP service has been removed; proof generation now runs inside `raiko-agent`.

## Deprecation Notice

The legacy `zisk-agent` service has been removed. Use `raiko-agent` for proof
requests, image management, and status polling.

## Contents

```
provers/zisk/agent/
├── driver/           # Raiko integration driver (active)
├── guest/            # ZISK guest programs
├── guest/elf/        # Compiled guest binaries (auto-generated)
├── build/            # Proof/publics output (auto-generated)
├── build.sh          # Build guest + driver
└── README.md         # This file
```

## Prerequisites

1. **ZISK Toolchain**: install cargo-zisk
   ```bash
   curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash
   source ~/.bashrc
   ```

2. **Rust Toolchain**: build script expects nightly-2024-12-20
   ```bash
   rustup toolchain install nightly-2024-12-20
   ```

3. **Optional - CUDA Toolkit**: for GPU acceleration
   ```bash
   sudo apt install nvidia-cuda-toolkit
   ```

4. **Optional - MPI**: for concurrent processing
   ```bash
   sudo apt install openmpi-bin openmpi-dev
   ```

## Building

### Build guest programs (ELF files)
```bash
./build.sh guest
```

### Build the Raiko driver
```bash
./build.sh driver
```

### Build everything (guest + driver)
```bash
./build.sh all
```

`./build.sh agent` is deprecated and will exit with a warning.

## Runtime Integration (raiko-agent)

1. Start `raiko-agent` (see the raiko-agent repo for details).
2. Configure the driver to point at raiko-agent:
   ```bash
   export ZISK_AGENT_URL="http://localhost:9999/proof"
   # or
   export RAIKO_AGENT_URL="http://localhost:9999/proof"
   export RAIKO_AGENT_API_KEY="..."   # optional
   ```
3. Upload the ZISK ELFs to raiko-agent:
   ```text
   POST /upload-image/zisk/batch
   POST /upload-image/zisk/aggregation
   ```

The driver will send proof requests to `POST /proof` and poll `GET /status/{request_id}`.

## Notes

- The driver embeds ELF bytes from `guest/elf/`, so rebuild guest programs before
  rebuilding the driver if the guest code changes.
- The legacy `zisk-agent` service was removed; use `raiko-agent` instead.
