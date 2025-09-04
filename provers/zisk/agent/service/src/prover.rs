use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::Hasher;
use std::process::Command;
use std::sync::{Arc, LazyLock};
use tempfile::TempDir;
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

// ELF binaries are loaded from relative paths (relative to target/release/ where binary runs)
const BATCH_ELF_PATH: &str = "../../guest/elf/zisk-batch";
const AGGREGATION_ELF_PATH: &str = "../../guest/elf/zisk-aggregation";

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
        // Create temporary directory for this proof
        let temp_dir = TempDir::new()?;
        let work_dir = temp_dir.path().join(&request_id);
        std::fs::create_dir_all(&work_dir)?;

        // Write input data
        let input_file = work_dir.join("input.bin");
        std::fs::write(&input_file, &input_data)?;

        // Ensure ROM setup
        ensure_rom_setup(BATCH_ELF_PATH).await?;

        // Generate proof
        let proof_dir = work_dir.join("proof");
        generate_proof_with_mpi(
            BATCH_ELF_PATH,
            input_file.to_str().unwrap(),
            proof_dir.to_str().unwrap(),
            self.config.concurrent_processes,
            self.config.threads_per_process,
        )?;

        // Read proof file
        let proof_file = proof_dir.join("vadcop_final_proof.bin");
        let proof_data = std::fs::read(&proof_file)?;
        let proof_hex = hex::encode(&proof_data);

        // Verify if requested
        if self.config.verify {
            verify_proof(&proof_file)?;
        }

        // Create response
        Ok(ZiskResponse {
            proof: Some(format!("0x{}", proof_hex)),
            receipt: Some("zisk_batch_receipt".to_string()),
            input: Some([0u8; 32]), // Simplified hash for now
            uuid: Some(request_id.to_string()),
        })
    }

    pub async fn aggregation_proof(&self, input_data: Vec<u8>) -> Result<ZiskResponse> {
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
        let result = self.execute_aggregation_proof(&input_data, &request_id).await;
        
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

    async fn execute_aggregation_proof(&self, input_data: &[u8], request_id: &str) -> Result<ZiskResponse> {

        // Create temporary directory
        let temp_dir = TempDir::new()?;
        let work_dir = temp_dir.path().join(&request_id);
        std::fs::create_dir_all(&work_dir)?;

        // Write input data
        let input_file = work_dir.join("input.bin");
        std::fs::write(&input_file, &input_data)?;

        // Ensure ROM setup
        ensure_rom_setup(AGGREGATION_ELF_PATH).await?;

        // Generate proof
        let proof_dir = work_dir.join("proof");
        generate_proof_with_mpi(
            AGGREGATION_ELF_PATH,
            input_file.to_str().unwrap(),
            proof_dir.to_str().unwrap(),
            self.config.concurrent_processes,
            self.config.threads_per_process,
        )?;

        // Read proof file
        let proof_file = proof_dir.join("vadcop_final_proof.bin");
        let proof_data = std::fs::read(&proof_file)?;
        let proof_hex = hex::encode(&proof_data);

        // Verify if requested
        if self.config.verify {
            verify_proof(&proof_file)?;
        }

        // Create response
        Ok(ZiskResponse {
            proof: Some(format!("0x{}", proof_hex)),
            receipt: Some("zisk_aggregation_receipt".to_string()),
            input: Some([0u8; 32]), // Simplified hash for now
            uuid: Some(request_id.to_string()),
        })
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

