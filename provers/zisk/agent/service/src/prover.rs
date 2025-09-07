use anyhow::{anyhow, Result};
use digest::Digest;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::Hasher;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, LazyLock};
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn, error};

use crate::types::{AggregationGuestInput, ZkAggregationGuestInput};

// ELF binaries are loaded from relative paths (relative to agent root directory)
const BATCH_ELF_PATH: &str = "guest/elf/zisk-batch";
const AGGREGATION_ELF_PATH: &str = "guest/elf/zisk-aggregation";

// Helper function to get absolute ELF paths
fn get_elf_path(relative: &str) -> String {
    // CARGO_MANIFEST_DIR points to the service directory at compile time
    // We need to go up one level to reach the agent directory
    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let elf_path = base_path.join(relative);
    
    // Convert to absolute path and log for debugging
    let absolute_path = std::fs::canonicalize(&elf_path)
        .unwrap_or_else(|_| elf_path.clone());
    
    info!("Resolved ELF path: {} -> {:?}", relative, absolute_path);
    absolute_path.to_string_lossy().into_owned()
}

// Proof cache structures
#[derive(Debug, Clone)]
enum ProofStatus {
    Pending,
    Completed(ZiskResponse),
    Failed(String),
}

#[derive(Debug, Clone)]
struct CachedProof {
    status: ProofStatus,
    #[allow(dead_code)]
    proof_type: String, // "batch" or "aggregation"
}

// Global state to coordinate ROM setup across concurrent requests
static ROM_SETUP_STATE: LazyLock<RomSetupCoordinator> = LazyLock::new(|| RomSetupCoordinator::new());

// Global proof cache to prevent duplicate requests
static PROOF_CACHE: LazyLock<Arc<Mutex<HashMap<u64, CachedProof>>>> = 
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

struct RomSetupCoordinator {
    completed: Mutex<HashSet<String>>,
    in_progress: Mutex<HashMap<String, Arc<Notify>>>,
}

impl RomSetupCoordinator {
    fn new() -> Self {
        Self {
            completed: Mutex::new(HashSet::new()),
            in_progress: Mutex::new(HashMap::new()),
        }
    }
}

// Helper function to calculate hash of input data
fn calculate_input_hash(input_data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(input_data);
    hasher.finish()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZiskProverConfig {
    pub verify: bool,
    pub concurrent_processes: Option<u32>,
    pub threads_per_process: Option<u32>,
}

impl Default for ZiskProverConfig {
    fn default() -> Self {
        Self {
            verify: true,
            concurrent_processes: None,
            threads_per_process: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZiskResponse {
    pub proof: Option<String>,
    pub receipt: Option<String>,
    pub input: Option<[u8; 32]>, // B256 equivalent
    pub uuid: Option<String>,
}

#[derive(Debug)]
pub struct ZiskProver {
    config: ZiskProverConfig,
}

impl ZiskProver {
    pub fn new(config: ZiskProverConfig) -> Self {
        Self { config }
    }

    pub async fn batch_proof(&self, input_data: Vec<u8>) -> Result<ZiskResponse> {
        // For batch proof, we pass the serialized GuestBatchInput directly to the guest
        // since the guest program expects the proper GuestBatchInput format
        info!("Received batch proof request with {} bytes of data", input_data.len());
        
        // Calculate hash for caching
        let input_hash = calculate_input_hash(&input_data);
        let request_id = format!("batch_{}", input_hash);
        
        info!("ZISK batch proof request with hash: {} (request_id: {})", input_hash, request_id);

        // Check cache first
        {
            let cache = PROOF_CACHE.lock().await;
            if let Some(cached_proof) = cache.get(&input_hash) {
                match &cached_proof.status {
                    ProofStatus::Pending => {
                        info!("Batch proof {} already in progress, returning error", input_hash);
                        return Err(anyhow!("Proof generation already in progress for this input"));
                    }
                    ProofStatus::Completed(response) => {
                        info!("Returning cached batch proof for {}", input_hash);
                        return Ok(response.clone());
                    }
                    ProofStatus::Failed(error) => {
                        warn!("Found cached failed proof for {}: {}", input_hash, error);
                        // Optionally retry by not returning here, or return cached error:
                        // return Err(anyhow!("Previous proof generation failed: {}", error));
                    }
                }
            }
        }

        // Mark as pending in cache
        {
            let mut cache = PROOF_CACHE.lock().await;
            cache.insert(input_hash, CachedProof {
                status: ProofStatus::Pending,
                proof_type: "batch".to_string(),
            });
        }

        info!("Starting ZISK batch proof generation with request_id: {}", request_id);

        // Execute proof generation with error handling  
        let result = self.execute_batch_proof(&input_data, &request_id).await;
        
        // Update cache based on result
        match &result {
            Ok(response) => {
                let mut cache = PROOF_CACHE.lock().await;
                cache.insert(input_hash, CachedProof {
                    status: ProofStatus::Completed(response.clone()),
                    proof_type: "batch".to_string(),
                });
                info!("Completed ZISK batch proof generation for {}", request_id);
            }
            Err(error) => {
                let mut cache = PROOF_CACHE.lock().await;
                cache.insert(input_hash, CachedProof {
                    status: ProofStatus::Failed(error.to_string()),
                    proof_type: "batch".to_string(),
                });
                warn!("Failed ZISK batch proof generation for {}: {}", request_id, error);
            }
        }

        result
    }

    async fn execute_batch_proof(&self, input_data: &[u8], request_id: &str) -> Result<ZiskResponse> {
        // Create persistent build directory for this proof
        let build_base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("build");
        std::fs::create_dir_all(&build_base)?;
        
        let work_dir = build_base.join(&request_id);
        std::fs::create_dir_all(&work_dir)?;
        
        info!("Using build directory: {:?}", work_dir);

        // Write the input data directly (it's already serialized GuestBatchInput from the driver)
        let input_file = work_dir.join("input.bin");
        std::fs::write(&input_file, input_data)?;
        info!("Wrote GuestBatchInput data to: {:?} (size: {} bytes)", input_file, input_data.len());

        // Ensure ROM setup
        let batch_elf_path = get_elf_path(BATCH_ELF_PATH);
        
        // Verify ELF file exists before proceeding
        if !std::path::Path::new(&batch_elf_path).exists() {
            return Err(anyhow!("Batch ELF file not found at: {}", batch_elf_path));
        }
        
        // Verify Zisk constraints before proof generation
        verify_zisk_constraints(&batch_elf_path, input_file.to_str().unwrap())?;
        
        ensure_rom_setup(&batch_elf_path).await?;

        // Generate proof
        let proof_dir = work_dir.join("proof");
        std::fs::create_dir_all(&proof_dir)?;
        info!("Generating proof in directory: {:?}", proof_dir);
        
        generate_proof_with_mpi(
            &batch_elf_path,
            input_file.to_str().unwrap(),
            proof_dir.to_str().unwrap(),
            self.config.concurrent_processes,
            self.config.threads_per_process,
        )?;

        // Read proof file
        let proof_file = proof_dir.join("vadcop_final_proof.bin");
        
        // Check if proof file exists
        if !proof_file.exists() {
            error!("Proof file not found at: {:?}", proof_file);
            error!("Contents of proof directory:");
            if let Ok(entries) = std::fs::read_dir(&proof_dir) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        error!("  - {:?}", entry.path());
                    }
                }
            }
            return Err(anyhow!("Proof file not generated at expected location: {:?}", proof_file));
        }
        
        info!("Reading proof file from: {:?}", proof_file);
        let proof_data = std::fs::read(&proof_file)?;
        info!("Read proof data: {} bytes", proof_data.len());
        let proof_hex = hex::encode(&proof_data);

        // Verify if requested
        if self.config.verify {
            verify_proof(&proof_file)?;
        }

        // Create response
        let response = ZiskResponse {
            proof: Some(format!("0x{}", proof_hex)),
            receipt: Some("zisk_batch_receipt".to_string()),
            input: Some([0u8; 32]), // Simplified hash for now
            uuid: Some(request_id.to_string()),
        };
        
        // Clean up build directory only after successful proof generation
        if let Err(e) = std::fs::remove_dir_all(&work_dir) {
            warn!("Failed to clean up build directory {}: {}", work_dir.display(), e);
        } else {
            info!("Cleaned up build directory: {:?}", work_dir);
        }
        
        Ok(response)
    }

    pub async fn aggregation_proof(&self, input_data: Vec<u8>) -> Result<ZiskResponse> {
        // Deserialize the input data to extract proof inputs for conversion to ZkAggregationGuestInput
        let aggregation_input: AggregationGuestInput = bincode::deserialize(&input_data)
            .map_err(|e| anyhow!("Failed to deserialize AggregationGuestInput: {e}"))?;
        
        info!("Received aggregation proof request with {} proofs", aggregation_input.proofs.len());
        
        // Calculate hash for caching
        let input_hash = calculate_input_hash(&input_data);
        let request_id = format!("aggregation_{}", input_hash);
        
        info!("ZISK aggregation proof request with hash: {} (request_id: {})", input_hash, request_id);

        // Check cache first
        {
            let cache = PROOF_CACHE.lock().await;
            if let Some(cached_proof) = cache.get(&input_hash) {
                match &cached_proof.status {
                    ProofStatus::Pending => {
                        info!("Aggregation proof {} already in progress, returning error", input_hash);
                        return Err(anyhow!("Proof generation already in progress for this input"));
                    }
                    ProofStatus::Completed(response) => {
                        info!("Returning cached aggregation proof for {}", input_hash);
                        return Ok(response.clone());
                    }
                    ProofStatus::Failed(error) => {
                        warn!("Found cached failed proof for {}: {}", input_hash, error);
                        // Optionally retry by not returning here, or return cached error:
                        // return Err(anyhow!("Previous proof generation failed: {}", error));
                    }
                }
            }
        }

        // Mark as pending in cache
        {
            let mut cache = PROOF_CACHE.lock().await;
            cache.insert(input_hash, CachedProof {
                status: ProofStatus::Pending,
                proof_type: "aggregation".to_string(),
            });
        }

        info!("Starting ZISK aggregation proof generation with request_id: {}", request_id);

        // Execute proof generation with error handling
        let result = self.execute_aggregation_proof(&aggregation_input, &request_id).await;
        
        // Update cache based on result
        match &result {
            Ok(response) => {
                let mut cache = PROOF_CACHE.lock().await;
                cache.insert(input_hash, CachedProof {
                    status: ProofStatus::Completed(response.clone()),
                    proof_type: "aggregation".to_string(),
                });
                info!("Completed ZISK aggregation proof generation for {}", request_id);
            }
            Err(error) => {
                let mut cache = PROOF_CACHE.lock().await;
                cache.insert(input_hash, CachedProof {
                    status: ProofStatus::Failed(error.to_string()),
                    proof_type: "aggregation".to_string(),
                });
                warn!("Failed ZISK aggregation proof generation for {}: {}", request_id, error);
            }
        }

        result
    }

    async fn execute_aggregation_proof(&self, aggregation_input: &AggregationGuestInput, request_id: &str) -> Result<ZiskResponse> {
        // Create persistent build directory for this proof
        let build_base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("build");
        std::fs::create_dir_all(&build_base)?;
        
        let work_dir = build_base.join(&request_id);
        std::fs::create_dir_all(&work_dir)?;
        
        info!("Using build directory: {:?}", work_dir);

        // Convert AggregationGuestInput to ZkAggregationGuestInput for ZISK
        let block_inputs: Vec<crate::types::B256> = aggregation_input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.input.ok_or_else(|| {
                    anyhow!(
                        "Proof {} input is None. Proof details: quote={:?}, uuid={:?}, proof_len={}", 
                        i,
                        proof.quote.as_ref().map(|q| format!("present, size:{}", q.len())),
                        proof.uuid,
                        proof.proof.as_ref().map(|p| p.len()).unwrap_or(0)
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Generate image ID from Zisk aggregation ELF hash  
        let aggregation_elf_path = get_elf_path(AGGREGATION_ELF_PATH);
        let elf_data = std::fs::read(&aggregation_elf_path)
            .map_err(|e| anyhow!("Failed to read aggregation ELF for image ID: {e}"))?;
        
        // Use keccak from our dependency instead of raiko_lib
        let mut hasher = sha3::Keccak256::new();
        hasher.update(&elf_data);
        let elf_hash = hasher.finalize();
        
        let mut image_id = [0u32; 8];
        for (i, chunk) in elf_hash.chunks(4).enumerate().take(8) {
            image_id[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        let zisk_input = ZkAggregationGuestInput {
            image_id,
            block_inputs: block_inputs.clone(),
        };

        // Write the ZkAggregationGuestInput data for the guest program
        let input_file = work_dir.join("input.bin");
        let serialized_input = bincode::serialize(&zisk_input)
            .map_err(|e| anyhow!("Failed to serialize ZkAggregationGuestInput for guest: {e}"))?;
        std::fs::write(&input_file, &serialized_input)?;
        info!("Wrote ZkAggregationGuestInput data to: {:?} (size: {} bytes, {} block inputs)", 
              input_file, serialized_input.len(), block_inputs.len());
        
        // Verify ELF file exists before proceeding
        if !std::path::Path::new(&aggregation_elf_path).exists() {
            return Err(anyhow!("Aggregation ELF file not found at: {}", aggregation_elf_path));
        }
        
        // Verify Zisk constraints before proof generation
        // verify_zisk_constraints(&aggregation_elf_path, input_file.to_str().unwrap())?;
        
        ensure_rom_setup(&aggregation_elf_path).await?;

        // Generate proof
        let proof_dir = work_dir.join("proof");
        std::fs::create_dir_all(&proof_dir)?;
        info!("Generating proof in directory: {:?}", proof_dir);
        
        generate_proof_with_mpi(
            &aggregation_elf_path,
            input_file.to_str().unwrap(),
            proof_dir.to_str().unwrap(),
            self.config.concurrent_processes,
            self.config.threads_per_process,
        )?;

        // Read proof file
        let proof_file = proof_dir.join("vadcop_final_proof.bin");
        
        // Check if proof file exists
        if !proof_file.exists() {
            error!("Proof file not found at: {:?}", proof_file);
            error!("Contents of proof directory:");
            if let Ok(entries) = std::fs::read_dir(&proof_dir) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        error!("  - {:?}", entry.path());
                    }
                }
            }
            return Err(anyhow!("Proof file not generated at expected location: {:?}", proof_file));
        }
        
        info!("Reading proof file from: {:?}", proof_file);
        let proof_data = std::fs::read(&proof_file)?;
        info!("Read proof data: {} bytes", proof_data.len());
        let proof_hex = hex::encode(&proof_data);

        // Verify if requested
        if self.config.verify {
            verify_proof(&proof_file)?;
        }

        // Create response
        let response = ZiskResponse {
            proof: Some(format!("0x{}", proof_hex)),
            receipt: Some("zisk_aggregation_receipt".to_string()),
            input: Some([0u8; 32]), // Simplified hash for now
            uuid: Some(request_id.to_string()),
        };
        
        // Clean up build directory only after successful proof generation
        if let Err(e) = std::fs::remove_dir_all(&work_dir) {
            warn!("Failed to clean up build directory {}: {}", work_dir.display(), e);
        } else {
            info!("Cleaned up build directory: {:?}", work_dir);
        }
        
        Ok(response)
    }
}

/// Run ROM setup only if it hasn't been done for this ELF yet
async fn ensure_rom_setup(elf_path: &str) -> Result<()> {
    let coordinator = &*ROM_SETUP_STATE;
    
    // Fast path: check if already completed
    {
        let completed = coordinator.completed.lock().await;
        if completed.contains(elf_path) {
            info!("ROM setup already completed for ELF: {}", elf_path);
            return Ok(());
        }
    }
    
    let notify_handle = {
        let mut in_progress = coordinator.in_progress.lock().await;
        
        // Check again if completed while waiting for lock
        {
            let completed = coordinator.completed.lock().await;
            if completed.contains(elf_path) {
                info!("ROM setup already completed for ELF: {}", elf_path);
                return Ok(());
            }
        }
        
        // Check if ROM setup is already in progress by another request
        if let Some(existing_notify) = in_progress.get(elf_path) {
            info!("ROM setup already in progress for ELF: {}, waiting...", elf_path);
            existing_notify.clone()
        } else {
            let notify = Arc::new(Notify::new());
            in_progress.insert(elf_path.to_string(), notify.clone());
            
            info!("Starting ROM setup for ELF: {} (first request)", elf_path);
            
            // Release the lock before running the blocking ROM setup command
            drop(in_progress);
            
            // Run the actual ROM setup command
            let rom_result = tokio::task::spawn_blocking({
                let elf_path = elf_path.to_string();
                move || {
                    Command::new("cargo-zisk")
                        .args(["rom-setup", "-e", &elf_path])
                        .output()
                        .map_err(|e| anyhow!("Zisk ROM setup failed: {}", e))
                }
            }).await;
            
            let rom_output = match rom_result {
                Ok(result) => result?,
                Err(e) => return Err(anyhow!("ROM setup task failed: {}", e)),
            };
            
            if !rom_output.status.success() {
                // ROM setup failed, clean up in_progress state
                coordinator.in_progress.lock().await.remove(elf_path);
                notify.notify_waiters();
                
                return Err(anyhow!(
                    "Zisk ROM setup failed: {}",
                    String::from_utf8_lossy(&rom_output.stderr)
                ));
            }
            
            // ROM setup succeeded, mark as completed
            {
                let mut completed = coordinator.completed.lock().await;
                completed.insert(elf_path.to_string());
            }
            
            // Clean up in_progress state and notify waiting requests
            coordinator.in_progress.lock().await.remove(elf_path);
            notify.notify_waiters();
            
            info!("ROM setup completed successfully for {}", elf_path);
            return Ok(());
        }
    };
    
    // Wait for ROM setup to complete by another request
    notify_handle.notified().await;
    
    // Check final result after waiting
    {
        let completed = coordinator.completed.lock().await;
        if completed.contains(elf_path) {
            info!("ROM setup completed by another request for ELF: {}", elf_path);
            Ok(())
        } else {
            Err(anyhow!("ROM setup failed for ELF: {}", elf_path))
        }
    }
}

/// Generate proof using MPI for concurrent execution if configured
fn generate_proof_with_mpi(
    elf_path: &str,
    input_path: &str,
    output_dir: &str,
    concurrent_processes: Option<u32>,
    threads_per_process: Option<u32>,
) -> Result<()> {
    let output = if let (Some(processes), Some(threads)) = (concurrent_processes, threads_per_process) {
        info!("Using MPI with {} processes, {} threads each", processes, threads);
        
        Command::new("mpirun")
            .args([
                "--bind-to", "none",
                "-np", &processes.to_string(),
                "-x", &format!("OMP_NUM_THREADS={}", threads),
                "-x", &format!("RAYON_NUM_THREADS={}", threads),
                "cargo-zisk", "prove",
                "-e", elf_path,
                "-i", input_path,
                "-o", output_dir,
                "-a"
            ])
            .output()?
    } else {
        Command::new("cargo-zisk")
            .args([
                "prove",
                "-e", elf_path,
                "-i", input_path,
                "-o", output_dir,
                "-a"
            ])
            .output()?
    };
    
    if !output.status.success() {
        return Err(anyhow!(
            "Zisk prove failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    // Log program output
    let stdout_output = String::from_utf8_lossy(&output.stdout);
    let stderr_output = String::from_utf8_lossy(&output.stderr);
    if !stdout_output.is_empty() {
        info!("Zisk program output: {}", stdout_output);
    }
    if !stderr_output.is_empty() {
        info!("Zisk program stderr: {}", stderr_output);
    }
    
    Ok(())
}

/// Verify proof using cargo-zisk verify
fn verify_proof(proof_file: &std::path::Path) -> Result<()> {
    let output = Command::new("cargo-zisk")
        .args(["verify", "-p", proof_file.to_str().unwrap()])
        .output()?;
    
    if !output.status.success() {
        return Err(anyhow!(
            "Zisk verification failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    let verify_stdout = String::from_utf8_lossy(&output.stdout);
    let verify_stderr = String::from_utf8_lossy(&output.stderr);
    if !verify_stdout.is_empty() {
        info!("Zisk verification output: {}", verify_stdout);
    }
    if !verify_stderr.is_empty() {
        info!("Zisk verification stderr: {}", verify_stderr);
    }
    
    Ok(())
}

fn verify_zisk_constraints(elf_path: &str, input_path: &str) -> Result<()> {
    info!("Verifying Zisk constraints for GuestBatchInput using cargo-zisk");

    // Get Zisk binary paths
    let witness_lib_path = std::env::var("HOME")
        .map(|home| format!("{}/.zisk/bin/libzisk_witness.so", home))
        .unwrap_or_else(|_| "$HOME/.zisk/bin/libzisk_witness.so".to_string());

    let proving_key_path = std::env::var("HOME")
        .map(|home| format!("{}/.zisk/provingKey", home))
        .unwrap_or_else(|_| "$HOME/.zisk/provingKey".to_string());

    // Run cargo-zisk verify-constraints command
    let output = Command::new("cargo-zisk")
        .args([
            "verify-constraints",
            "-e", elf_path,
            "-i", input_path,
            "-w", &witness_lib_path,
            "-k", &proving_key_path,
        ])
        .output()
        .map_err(|e| anyhow!("Failed to run cargo-zisk verify-constraints: {e}"))?;

    // Check if verification succeeded
    if output.status.success() {
        info!("Zisk constraints verification PASSED");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            info!("Verification output:\n{}", stdout);
        }
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        info!("Zisk constraints verification FAILED");
        if !stdout.is_empty() {
            info!("Verification stdout:\n{}", stdout);
        }
        if !stderr.is_empty() {
            error!("Verification stderr:\n{}", stderr);
        }

        Err(anyhow!("Zisk constraints verification failed: {}", stderr))
    }
}

