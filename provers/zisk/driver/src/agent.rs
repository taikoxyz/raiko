use once_cell::sync::Lazy;

use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput, ShastaZiskAggregationGuestInput,
        ZkAggregationGuestInput,
    },
    prover::{Proof, ProofKey},
};
use raiko_lib::{
    libhash::hash_shasta_subproof_input,
    primitives::{Address, B256},
    proof_type::ProofType as RaikoProofType,
    prover::{IdStore, IdWrite, Prover, ProverConfig, ProverError, ProverResult},
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tracing::{info, warn};

use zisk_common::ElfBinaryFromFile;
use zisk_sdk::{
    Asm, ProofOpts, ProverClientBuilder, ZiskProgramPK, ZiskProgramVK, ZiskProof, ZiskProveResult,
    ZiskProver,
};

/// Locate the directory containing guest ELFs.
///
/// Search order:
///   1. Same directory as the running binary (`<exe_dir>/`).
///      Works for installed/Docker deployments where ELFs are copied
///      alongside the binary.
///   2. Compile-time fallback: `CARGO_MANIFEST_DIR/../guest/elf`
///      Works for `cargo run` / dev builds where the elf dir sits next to
///      the driver crate in the source tree.
fn find_elf_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            let candidate = bin_dir.join("zisk-batch");
            if candidate.exists() {
                return bin_dir.to_path_buf();
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../guest/elf"))
}

static ELF_DIR: Lazy<PathBuf> = Lazy::new(find_elf_dir);

fn elf_path(elf_name: &str) -> PathBuf {
    ELF_DIR.join(elf_name)
}

// ---------------------------------------------------------------------------
// Local proving config
// ---------------------------------------------------------------------------

struct ZiskLocalConfig {
    proving_key: PathBuf,
    proving_key_snark: PathBuf,
    output_dir: PathBuf,
}

impl ZiskLocalConfig {
    fn from_env() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        Self {
            proving_key: std::env::var("ZISK_PROVING_KEY")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(format!("{home}/.zisk/provingKey"))),
            proving_key_snark: std::env::var("ZISK_PROVING_KEY_SNARK")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(format!("{home}/.zisk/provingKeySnark"))),
            output_dir: std::env::var("ZISK_OUTPUT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/tmp/zisk-proofs")),
        }
    }
}

// ---------------------------------------------------------------------------
// Cached prover instances
// ---------------------------------------------------------------------------
//
// Building a ZiskProver starts ASM microservices (~19s) and initializes
// proofman (~12s). Since prove() and setup() take &self, provers are
// reusable. We cache one in a static so the first proof pays the init
// cost and all subsequent proofs skip it entirely (~31s saved per proof).
//
// NOTE: MPI can only be initialized once per process (ProofMan::new calls
// MpiCtx::new). We must use a **single prover** for everything. Building
// with .snark() gives us a superset: it can run both STARK-only proofs
// (.run()) and SNARK proofs (.plonk().run()).

static PROVER: Lazy<ZiskProver<Asm>> = Lazy::new(|| {
    info!("Building prover (first call — will be cached for reuse)");

    // Register atexit handler to stop ASM microservices on process exit.
    // Statics are never dropped in Rust, so without this the child processes
    // (mo, mt, rh) would be orphaned.
    unsafe { libc::atexit(shutdown_asm_on_exit) };

    let config = ZiskLocalConfig::from_env();
    ProverClientBuilder::new()
        .asm()
        .prove()
        .aggregation(true)
        .snark()
        .proving_key_path(config.proving_key)
        .proving_key_snark_path(config.proving_key_snark)
        .verbose(0)
        .shared_tables(true)
        .unlock_mapped_memory(false)
        .print_command_info()
        .build::<fields::Goldilocks>()
        .expect("Failed to build prover")
});

fn setup_elf(elf_name: &str) -> (ZiskProgramPK, ZiskProgramVK) {
    info!(
        "Setting up {} (first call — includes ROM setup + ASM services)",
        elf_name
    );
    let elf = elf_path(elf_name);
    let elf_binary = ElfBinaryFromFile::new(&elf, false).expect("Failed to read ELF");
    PROVER
        .setup(&elf_binary)
        .unwrap_or_else(|e| panic!("Failed to setup {elf_name}: {e}"))
}

// Per-ELF proving key (PK) + verification key (VK) caches.
// Each Lazy triggers setup() on first access, starting ASM microservices
// for that ELF. In production, get_guest_data() warms all 3 at startup.
static BATCH_PK: Lazy<(ZiskProgramPK, ZiskProgramVK)> = Lazy::new(|| setup_elf("zisk-batch"));
static AGG_PK: Lazy<(ZiskProgramPK, ZiskProgramVK)> = Lazy::new(|| setup_elf("zisk-aggregation"));
static SHASTA_AGG_PK: Lazy<(ZiskProgramPK, ZiskProgramVK)> =
    Lazy::new(|| setup_elf("zisk-shasta-aggregation"));

fn cached_pk(elf_name: &str) -> &'static (ZiskProgramPK, ZiskProgramVK) {
    match elf_name {
        "zisk-batch" => &BATCH_PK,
        "zisk-aggregation" => &AGG_PK,
        "zisk-shasta-aggregation" => &SHASTA_AGG_PK,
        _ => panic!("Unknown ELF: {elf_name}"),
    }
}

/// Get the vkey hex for an ELF, derived from the cached PK/VK.
/// In production, get_guest_data() warms all 3 ELFs at startup,
/// so subsequent calls are just pointer dereferences.
fn cached_vkey_hex(elf_name: &str) -> String {
    let (_pk, vk) = cached_pk(elf_name);
    // Swap byte order within each 8-byte (uint64) word: zisk stores vkey as
    // LE uint64 values, but the on-chain verifier expects BE uint64 layout.
    let mut swapped = vk.vk.clone();
    for chunk in swapped.chunks_exact_mut(8) {
        chunk.reverse();
    }
    hex::encode(&swapped)
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

extern "C" fn shutdown_asm_on_exit() {
    shutdown_zisk();
}

/// Stop ASM microservices by killing processes on the known ports.
/// Called automatically via atexit, but can also be called explicitly
/// for graceful server shutdown.
///
/// We don't use zisk's `stop_asm_services()` because it calls
/// `tracing::info!` internally, which panics on TLS access during atexit.
/// Instead we kill the processes directly via `fuser`.
///
/// Port assignment in zisk SDK depends on the order of setup() calls,
/// not a fixed mapping. With up to 3 setups × 3 services (mo/mt/rh),
/// we scan the full range BASE_PORT..BASE_PORT+9 and kill anything listening.
pub fn shutdown_zisk() {
    eprintln!("[zisk] Stopping ASM microservices");
    // ASM service ports from zisk SDK's AsmServices:
    //   base_port = 23115 (ASM_SERVICE_BASE_PORT)
    //   Each setup() allocates 3 consecutive ports (mo +0, mt +1, rh +2)
    //   Max 3 setups (batch, aggregation, shasta) = 9 ports total
    const BASE_PORT: u16 = 23115;
    const MAX_SETUPS: u16 = 3;
    const SERVICES_PER_SETUP: u16 = 3; // mo, mt, rh
    const SERVICE_NAMES: [&str; 3] = ["mo", "mt", "rh"];

    for i in 0..(MAX_SETUPS * SERVICES_PER_SETUP) {
        let port = BASE_PORT + i;
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            let svc_name = SERVICE_NAMES[(i % SERVICES_PER_SETUP) as usize];
            let _ = std::process::Command::new("fuser")
                .args(["-k", &format!("{port}/tcp")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            eprintln!("[zisk] Killed {svc_name} service on port {port}");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a ZiskStdin with properly framed data.
///
/// v0.16.0 requires input data to be framed with 8-byte length prefixes
/// and 8-byte alignment. `from_vec` passes raw bytes (no framing), so we
/// must use `write_slice` which adds the proper framing.
fn build_zisk_stdin(serialized_input: Vec<u8>) -> zisk_common::io::ZiskStdin {
    let stdin = zisk_common::io::ZiskStdin::new();
    stdin.write_slice(&serialized_input);
    stdin
}

fn save_input(elf_name: &str, stdin: &zisk_common::io::ZiskStdin, config: &ZiskLocalConfig) {
    std::fs::create_dir_all(&config.output_dir).ok();
    let input_path = config.output_dir.join(format!("{elf_name}-input.bin"));
    // Save the framed data so CLI --input can read it directly
    stdin.save(&input_path).ok();
    info!("Saved {} framed input to {:?}", elf_name, input_path);
}

fn prove_stark(elf_name: &str, serialized_input: Vec<u8>) -> ProverResult<ZiskProveResult> {
    let config = ZiskLocalConfig::from_env();
    info!("Using ELF at {:?}", elf_path(elf_name));

    let (pk, _vk) = cached_pk(elf_name);

    let stdin = build_zisk_stdin(serialized_input);
    save_input(elf_name, &stdin, &config);

    let proof_opts = ProofOpts::default()
        .output_dir(config.output_dir)
        .verify_proofs();

    let result = PROVER
        .prove(pk, stdin)
        .with_proof_options(proof_opts)
        .run()
        .map_err(|e| ProverError::GuestError(format!("Zisk STARK proof failed: {e}")))?;

    Ok(result)
}

fn prove_stark_with_snark(
    elf_name: &str,
    serialized_input: Vec<u8>,
) -> ProverResult<ZiskProveResult> {
    let config = ZiskLocalConfig::from_env();
    info!("Using ELF at {:?}", elf_path(elf_name));

    let (pk, _vk) = cached_pk(elf_name);

    let stdin = build_zisk_stdin(serialized_input);
    save_input(elf_name, &stdin, &config);

    let proof_opts = ProofOpts::default()
        .output_dir(config.output_dir)
        .verify_proofs();

    let result = PROVER
        .prove(pk, stdin)
        .plonk()
        .with_proof_options(proof_opts)
        .run()
        .map_err(|e| ProverError::GuestError(format!("Zisk SNARK proof failed: {e}")))?;

    Ok(result)
}

fn zisk_proof_to_bytes(proof: &ZiskProof) -> ProverResult<Vec<u8>> {
    match proof {
        ZiskProof::VadcopFinal(bytes) | ZiskProof::VadcopFinalCompressed(bytes) => {
            Ok(bytes.clone())
        }
        ZiskProof::Plonk(bytes) | ZiskProof::Fflonk(bytes) => Ok(bytes.clone()),
        ZiskProof::Null() => Err(ProverError::GuestError("Proof is Null".into())),
    }
}

fn stark_proof_to_raiko_proof(
    result: &ZiskProveResult,
    vkey: Option<String>,
    inner_vkey: Option<&str>,
) -> ProverResult<Proof> {
    let proof_bytes = zisk_proof_to_bytes(result.get_proof())?;
    // Prepend vkey(s) to proof bytes, matching SP1 format:
    //   batch:  "0x" + vkey_hex + proof_hex
    //   agg:    "0x" + vkey_hex + inner_vkey_hex + proof_hex
    let vk_hex = vkey
        .as_deref()
        .map(|v| v.strip_prefix("0x").unwrap_or(v))
        .unwrap_or("");
    let inner_hex = inner_vkey
        .map(|v| v.strip_prefix("0x").unwrap_or(v))
        .unwrap_or("");
    let proof_string = format!("0x{}{}{}", vk_hex, inner_hex, hex::encode(&proof_bytes));

    Ok(Proof {
        proof: Some(proof_string),
        quote: None,
        input: Some(
            result.get_publics().public_bytes_solidity()[..32]
                .try_into()
                .expect("Expected 32 bytes for output hash"),
        ),
        uuid: vkey,
        kzg_proof: None,
        extra_data: None,
    })
}

fn snark_proof_to_raiko_proof(
    result: &ZiskProveResult,
    vkey: Option<String>,
    inner_vkey: Option<&str>,
) -> ProverResult<Proof> {
    let proof_bytes = zisk_proof_to_bytes(result.get_proof())?;
    // For SNARK proofs, publics are encoded separately
    let program_vk = result.get_program_vk();
    // Prepend vkey(s) to proof bytes, matching SP1 format:
    //   batch:  "0x" + vkey_hex + proof_hex
    //   agg:    "0x" + vkey_hex + inner_vkey_hex + proof_hex
    let vk_hex = vkey
        .as_deref()
        .map(|v| v.strip_prefix("0x").unwrap_or(v))
        .unwrap_or("");
    let inner_hex = inner_vkey
        .map(|v| v.strip_prefix("0x").unwrap_or(v))
        .unwrap_or("");
    let proof_string = format!("0x{}{}{}", vk_hex, inner_hex, hex::encode(&proof_bytes));
    Ok(Proof {
        proof: Some(proof_string),
        quote: Some(hex::encode(&program_vk.vk)),
        input: Some(
            result.get_publics().public_bytes_solidity()[..32]
                .try_into()
                .expect("Expected 32 bytes for output hash"),
        ),
        uuid: vkey,
        kzg_proof: None,
        extra_data: None,
    })
}

fn vkey_to_image_id(vkey_hex: &str) -> [u32; 8] {
    let bytes =
        hex::decode(vkey_hex.strip_prefix("0x").unwrap_or(vkey_hex)).expect("invalid vkey hex");

    assert!(
        bytes.len() == 32,
        "Expected 32 bytes for vkey, got {}",
        bytes.len()
    );
    let mut image_id = [0u32; 8];

    for (i, chunk) in bytes.chunks(4).enumerate().take(8) {
        image_id[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    image_id
}

/// Get the zisk batch vkey string.
///
/// In production, batch proofs carry the vkey in their `uuid` field (set by
/// `batch_run`). Using it avoids triggering `BATCH_PK` setup, which starts a
/// separate set of ASM microservices and causes GPU contention with the
/// aggregation prover.
///
/// Falls back to `cached_vkey_hex("zisk-batch")` (triggering setup) when:
/// - `uuid` is absent, or
/// - `uuid` is not a valid 32-byte hex string (e.g. SP1 JSON vkey in fixtures).
fn zisk_batch_vkey_from_proofs(proofs: &[raiko_lib::prover::Proof]) -> String {
    if let Some(uuid) = proofs.first().and_then(|p| p.uuid.as_deref()) {
        let stripped = uuid.strip_prefix("0x").unwrap_or(uuid);
        if let Ok(bytes) = hex::decode(stripped) {
            if bytes.len() == 32 {
                return uuid.to_string();
            }
        }
    }
    warn!("Batch proof missing valid vkey in uuid, falling back to cached vkey (triggers setup)");
    cached_vkey_hex("zisk-batch")
}

// ---------------------------------------------------------------------------
// Prover
// ---------------------------------------------------------------------------

pub struct ZiskAgentProver;

impl ZiskAgentProver {
    pub async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("no block run after pacaya fork")
    }

    pub async fn batch_run(
        &self,
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let batch_snark = config
            .get("zisk")
            .and_then(|z| z.get("batch_snark"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        info!(
            "Zisk local batch proof starting (mode: {})",
            if batch_snark { "snark" } else { "stark" }
        );

        let serialized_input = bincode::serialize(&input).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize GuestBatchInput: {e}"))
        })?;

        let proof = tokio::task::spawn_blocking(move || {
            let batch_vkey = cached_vkey_hex("zisk-batch");
            if batch_snark {
                let result = prove_stark_with_snark("zisk-batch", serialized_input)?;
                snark_proof_to_raiko_proof(&result, Some(batch_vkey), None)
            } else {
                let result = prove_stark("zisk-batch", serialized_input)?;
                stark_proof_to_raiko_proof(&result, Some(batch_vkey), None)
            }
        })
        .await
        .map_err(|e| ProverError::GuestError(format!("spawn_blocking failed: {e}")))??;

        Ok(proof)
    }

    pub async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        info!("Zisk local aggregation proof starting");

        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.input.ok_or_else(|| {
                    ProverError::GuestError(format!("Proof {} input is None for aggregation", i))
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;

        let batch_vkey = zisk_batch_vkey_from_proofs(&input.proofs);
        let zisk_input = ZkAggregationGuestInput {
            image_id: vkey_to_image_id(&batch_vkey),
            block_inputs,
        };
        let serialized_input = bincode::serialize(&zisk_input).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize aggregation input: {e}"))
        })?;

        let proof = tokio::task::spawn_blocking(move || {
            let agg_vkey = cached_vkey_hex("zisk-aggregation");
            let result = prove_stark_with_snark("zisk-aggregation", serialized_input)?;
            snark_proof_to_raiko_proof(&result, Some(agg_vkey), Some(&batch_vkey))
        })
        .await
        .map_err(|e| ProverError::GuestError(format!("spawn_blocking failed: {e}")))??;

        Ok(proof)
    }

    pub async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        info!("Zisk local shasta aggregation proof starting");

        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.input.ok_or_else(|| {
                    ProverError::GuestError(format!(
                        "Proof {} input is None for shasta aggregation",
                        i
                    ))
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;

        let proof_carry_data_vec = input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.extra_data.clone().ok_or_else(|| {
                    ProverError::GuestError(format!("Proof {} missing shasta proof carry data", i))
                })
            })
            .collect::<ProverResult<Vec<_>>>()?;

        if block_inputs.len() != proof_carry_data_vec.len() {
            return Err(ProverError::GuestError(format!(
                "Shasta aggregation input length mismatch: {} block inputs vs {} carry records",
                block_inputs.len(),
                proof_carry_data_vec.len()
            )));
        }

        for (i, block_input) in block_inputs.iter().enumerate() {
            let expected = hash_shasta_subproof_input(&proof_carry_data_vec[i]);
            if *block_input != expected {
                return Err(ProverError::GuestError(format!(
                    "Shasta aggregation block input {} does not match proof carry data",
                    i
                )));
            }
        }

        let batch_vkey = zisk_batch_vkey_from_proofs(&input.proofs);
        let shasta_input = ShastaZiskAggregationGuestInput {
            image_id: vkey_to_image_id(&batch_vkey),
            block_inputs,
            proof_carry_data_vec,
            prover_address: Address::ZERO,
        };
        let serialized_input = bincode::serialize(&shasta_input).map_err(|e| {
            ProverError::GuestError(format!("Failed to serialize shasta input: {e}"))
        })?;

        let proof = tokio::task::spawn_blocking(move || {
            let shasta_vkey = cached_vkey_hex("zisk-shasta-aggregation");
            let result = prove_stark_with_snark("zisk-shasta-aggregation", serialized_input)?;
            snark_proof_to_raiko_proof(&result, Some(shasta_vkey), Some(&batch_vkey))
        })
        .await
        .map_err(|e| ProverError::GuestError(format!("spawn_blocking failed: {e}")))??;

        Ok(proof)
    }

    pub async fn cancel(
        &self,
        _proof_key: ProofKey,
        _id_store: Box<&mut dyn IdStore>,
    ) -> ProverResult<()> {
        info!("Zisk agent cancel requested - not implemented");
        Ok(())
    }
}

impl Prover for ZiskAgentProver {
    async fn get_guest_data() -> ProverResult<serde_json::Value> {
        // This initializes provers + runs ROM setup on first call.
        // All subsequent calls return cached results instantly.
        let data = tokio::task::spawn_blocking(|| -> ProverResult<serde_json::Value> {
            let batch_vkey = cached_vkey_hex("zisk-batch");
            Ok(json!({
                "zisk": {
                    "batch_vkey": batch_vkey,
                }
            }))
        })
        .await
        .map_err(|e| ProverError::GuestError(format!("spawn_blocking failed: {e}")))??;

        Ok(data)
    }

    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        ZiskAgentProver::run(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        ZiskAgentProver::batch_run(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        ZiskAgentProver::aggregate(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn shasta_aggregate(
        &self,
        input: raiko_lib::input::ShastaAggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        ZiskAgentProver::shasta_aggregate(self, input, output, config, None)
            .await
            .map_err(Into::into)
    }

    async fn cancel(&self, _proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    fn proof_type(&self) -> RaikoProofType {
        RaikoProofType::Zisk
    }
}
