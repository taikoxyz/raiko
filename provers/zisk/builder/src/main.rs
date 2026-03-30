use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    println!("=== Building ZISK guest programs ===");

    let builder_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let guest_dir = builder_dir
        .parent()
        .context("builder has no parent dir")?
        .join("guest");

    println!("Guest dir: {}", guest_dir.display());

    if !guest_dir.join("Cargo.toml").exists() {
        bail!("ZISK guest Cargo.toml not found at {}", guest_dir.display());
    }

    // 1. Find ZISK_RUSTC
    let zisk_rustc = find_zisk_rustc()?;
    println!("Using ZISK rustc: {}", zisk_rustc.display());

    // 2. Detect riscv toolchain and sysroot
    let (riscv_gcc, sysroot) = detect_riscv_tools();

    // 3. Build guest programs using the ZisK custom rustc
    build_guest(&guest_dir, &zisk_rustc, &riscv_gcc, sysroot.as_deref())?;

    // 4. Copy ELFs to guest/elf/
    let elf_dir = guest_dir.join("elf");
    fs::create_dir_all(&elf_dir)?;

    let elf_source_dir = guest_dir.join("target/riscv64ima-zisk-zkvm-elf/release");
    for elf_name in &["zisk-batch", "zisk-aggregation", "zisk-shasta-aggregation"] {
        let src = elf_source_dir.join(elf_name);
        if !src.exists() {
            bail!("{} ELF not found at {}", elf_name, src.display());
        }
        let dst = elf_dir.join(elf_name);
        fs::copy(&src, &dst).with_context(|| format!("Failed to copy {elf_name} ELF"))?;
        println!("Copied {elf_name} to {}", dst.display());
    }

    // 5. Print zisk-batch vkey (requires proving key to be installed)
    print_batch_vkey(&elf_dir.join("zisk-batch"));

    println!("=== ZISK guest programs built successfully ===");
    Ok(())
}

/// Find the ZisK custom rustc binary.
/// Search order:
///   1. ZISK_RUSTC env var (explicit override)
///   2. $ZISK_TOOLCHAIN_DIR/bin/rustc
///   3. $HOME/.zisk/toolchains/*/bin/rustc (installed via cargo-zisk sdk install-toolchain)
fn find_zisk_rustc() -> Result<PathBuf> {
    // 1. Explicit override
    if let Ok(rustc) = std::env::var("ZISK_RUSTC") {
        let path = PathBuf::from(&rustc);
        if path.exists() {
            return Ok(path);
        }
        bail!("ZISK_RUSTC is set to '{rustc}' but the file does not exist");
    }

    // 2. ZISK_TOOLCHAIN_DIR
    if let Ok(tc_dir) = std::env::var("ZISK_TOOLCHAIN_DIR") {
        let path = PathBuf::from(&tc_dir).join("bin/rustc");
        if path.exists() {
            return Ok(path);
        }
    }

    // 3. ~/.zisk/toolchains/*/bin/rustc
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let zisk_dir = std::env::var("ZISK_DIR").unwrap_or_else(|_| format!("{home}/.zisk"));
    let toolchains_dir = PathBuf::from(&zisk_dir).join("toolchains");

    if toolchains_dir.exists() {
        for entry in fs::read_dir(&toolchains_dir)
            .with_context(|| format!("Failed to read {}", toolchains_dir.display()))?
        {
            let rustc = entry?.path().join("bin/rustc");
            if rustc.exists() {
                return Ok(rustc);
            }
        }
    }

    bail!(
        "ZisK Rust toolchain not found.\n  \
         Install via: cargo-zisk sdk install-toolchain\n  \
         Or set ZISK_RUSTC=/path/to/zisk/rustc"
    )
}

/// Detect riscv C toolchain and sysroot for C dependencies.
/// Returns (gcc_binary, optional_sysroot_include_path).
/// Preference order:
///   1. SP1 bundled gcc (~/.sp1/riscv/bin/riscv64-unknown-elf-gcc)
///   2. SP1 newlib headers directly (~/.sp1/riscv/riscv64-unknown-elf/include)
///   3. System riscv64-unknown-elf-gcc sysroot
fn detect_riscv_tools() -> (String, Option<String>) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let sp1_gcc = format!("{home}/.sp1/riscv/bin/riscv64-unknown-elf-gcc");
    let sp1_include = format!("{home}/.sp1/riscv/riscv64-unknown-elf/include");

    // 1. SP1 bundled gcc
    if PathBuf::from(&sp1_gcc).exists() {
        if Command::new(&sp1_gcc)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            if let Ok(out) = Command::new(&sp1_gcc).arg("-print-sysroot").output() {
                let base = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let sysroot = format!("{base}/include");
                println!("Using SP1 bundled gcc sysroot: {sysroot}");
                return (sp1_gcc, Some(sysroot));
            }
            return (sp1_gcc, None);
        }
    }

    // 2. SP1 newlib headers
    if PathBuf::from(&sp1_include).exists()
        && fs::read_dir(&sp1_include)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
    {
        println!("Using SP1 newlib headers: {sp1_include}");
        return ("riscv64-unknown-elf-gcc".to_string(), Some(sp1_include));
    }

    // 3. System riscv64-unknown-elf-gcc
    if let Ok(out) = Command::new("riscv64-unknown-elf-gcc")
        .arg("-print-sysroot")
        .output()
    {
        if out.status.success() {
            let base = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let sysroot = format!("{base}/include");
            if PathBuf::from(&sysroot).exists() {
                println!("Using system riscv64 sysroot: {sysroot}");
                return ("riscv64-unknown-elf-gcc".to_string(), Some(sysroot));
            } else {
                // sysroot exists but no include dir — fall back to SP1 headers
                println!(
                    "System gcc has no sysroot include, falling back to SP1 headers: {sp1_include}"
                );
                return ("riscv64-unknown-elf-gcc".to_string(), Some(sp1_include));
            }
        }
    }

    eprintln!(
        "Warning: No riscv sysroot found. C dependencies may fail to compile.\n  \
         Install SP1 toolchain (sp1up) or riscv64-unknown-elf-gcc."
    );
    ("riscv64-unknown-elf-gcc".to_string(), None)
}

/// Run `cargo build --target riscv64ima-zisk-zkvm-elf --release` inside the guest dir.
fn build_guest(
    guest_dir: &PathBuf,
    zisk_rustc: &PathBuf,
    riscv_gcc: &str,
    sysroot: Option<&str>,
) -> Result<()> {
    println!("Building with ZISK rustc for riscv64ima-zisk-zkvm-elf target...");
    println!("RUSTC={}", zisk_rustc.display());

    let cc_flag =
        format!("{riscv_gcc} -march=rv64ima -mabi=lp64 -mstrict-align -falign-functions=2");

    let mut cmd = Command::new("cargo");
    cmd.current_dir(guest_dir);
    cmd.env("RUSTC", zisk_rustc);
    cmd.env("CC_riscv64ima_zisk_zkvm_elf", &cc_flag);
    cmd.env("RUSTFLAGS", "--cfg getrandom_backend=\"custom\"");
    cmd.env_remove("TARGET_CC");

    if let Some(sr) = sysroot {
        cmd.env("CFLAGS_riscv64ima_zisk_zkvm_elf", format!("-isystem {sr}"));
    }

    // Remove host CARGO_* env vars to avoid interference
    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO") {
            cmd.env_remove(&key);
        }
    }

    cmd.args(["build", "--target", "riscv64ima-zisk-zkvm-elf", "--release"]);

    let status = cmd.status().context("Failed to spawn cargo build")?;
    if !status.success() {
        bail!("cargo build failed with exit code: {status}");
    }

    Ok(())
}

/// Compute and print the zisk-batch verification key.
/// Requires the ZisK proving key to be installed at ZISK_PROVING_KEY or ~/.zisk/provingKey.
///
/// Fast path: reads the cached verkey file from ~/.zisk/cache/ by matching the blake3 hash
/// of the ELF binary (no native code, pure file I/O).
/// Slow path: runs `cargo-zisk rom-setup` to populate the cache, then re-reads.
///
/// Uses only blake3 (pure Rust) — intentionally avoids zisk-sdk to prevent linking
/// proofman-starks native C++ which requires AVX-512 and causes SIGILL on non-AVX-512 hosts.
fn print_batch_vkey(batch_elf_path: &PathBuf) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let proving_key = std::env::var("ZISK_PROVING_KEY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("{home}/.zisk/provingKey")));

    if !proving_key.exists() {
        eprintln!(
            "Warning: proving key not found at {}. Skipping vkey computation.\n  \
             Set ZISK_PROVING_KEY or install via: cargo-zisk sdk install-setup",
            proving_key.display()
        );
        return;
    }

    let elf_bytes = match fs::read(batch_elf_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Warning: failed to read batch ELF for vkey: {e}");
            return;
        }
    };
    let elf_hash = blake3::hash(&elf_bytes).to_hex().to_string();

    let cache_dir = PathBuf::from(format!("{home}/.zisk/cache"));

    // Fast path: verkey already cached (e.g. from a previous make guest or prove run).
    if let Some(verkey_bytes) = read_cached_verkey(&cache_dir, &elf_hash) {
        print_vk_bytes(&verkey_bytes);
        return;
    }

    // Slow path: run rom-setup to compute and cache the verkey (~12s first time).
    println!("ROM cache not found — running setup to compute vkey (this may take ~12s)...");
    println!(
        "Running: cargo-zisk rom-setup -e {}",
        batch_elf_path.display()
    );
    let status = Command::new("cargo-zisk")
        .args([
            "rom-setup",
            "-e",
            batch_elf_path.to_str().unwrap_or_default(),
            "-k",
            proving_key.to_str().unwrap_or_default(),
        ])
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("Warning: cargo-zisk rom-setup failed with exit code: {s}");
            return;
        }
        Err(e) => {
            eprintln!("Warning: failed to run cargo-zisk rom-setup: {e}");
            eprintln!(
                "  Install cargo-zisk or run manually: cargo-zisk rom-setup -e {} -k {}",
                batch_elf_path.display(),
                proving_key.display()
            );
            return;
        }
    }

    // Re-read the cache that rom-setup just populated.
    match read_cached_verkey(&cache_dir, &elf_hash) {
        Some(verkey_bytes) => print_vk_bytes(&verkey_bytes),
        None => eprintln!(
            "Warning: verkey cache not found under {} after rom-setup",
            cache_dir.display()
        ),
    }
}

/// Find and read a cached verkey file whose name starts with the given ELF hash.
/// zisk stores verkeys as: {elf_hash}_{pil_hash}_{rows}_{blowup}_{arity}.verkey.bin
fn read_cached_verkey(cache_dir: &PathBuf, elf_hash: &str) -> Option<Vec<u8>> {
    let entries = fs::read_dir(cache_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with(elf_hash) && name.ends_with(".verkey.bin") {
            return fs::read(&path).ok();
        }
    }
    None
}

/// Byte-swap within each 8-byte (uint64) word and print as hex.
/// ZisK stores vkey as LE uint64 values; the on-chain verifier expects BE uint64 layout.
fn print_vk_bytes(vk: &[u8]) {
    let mut swapped = vk.to_vec();
    for chunk in swapped.chunks_exact_mut(8) {
        chunk.reverse();
    }
    println!("zisk-batch vkey: {}", hex::encode(&swapped));
}
