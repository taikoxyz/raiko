#![cfg(feature = "enable")]

use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ZkAggregationGuestInput,
    },
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
    protocol_instance::{aggregation_output, words_to_bytes_le},
    primitives::keccak::keccak,
    Measurement,
};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, LazyLock};
use tokio::sync::{Mutex, Notify};
use tracing::info;

pub const BATCH_ELF: &[u8] = include_bytes!("../../guest/elf/zisk-batch");
pub const AGGREGATION_ELF: &[u8] = include_bytes!("../../guest/elf/zisk-aggregation");

// Global state to coordinate ROM setup across concurrent requests
static ROM_SETUP_STATE: LazyLock<RomSetupCoordinator> = LazyLock::new(|| RomSetupCoordinator::new());

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

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZiskParam {
    pub prover: Option<ProverMode>,
    #[serde(default = "DEFAULT_TRUE")]
    pub verify: bool,
    // New options for concurrent proof generation
    #[serde(default)]
    pub concurrent_processes: Option<u32>,
    #[serde(default)]
    pub threads_per_process: Option<u32>,
    #[serde(default)]
    /// Enable individual proof verification on host before aggregation
    pub host_verification: Option<bool>,
}

const DEFAULT_TRUE: fn() -> bool = || true;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProverMode {
    Local,
    Remote,
}



impl From<ZiskResponse> for Proof {
    fn from(value: ZiskResponse) -> Self {
        Self {
            proof: value.proof,
            quote: value.receipt,
            input: value.input,
            uuid: value.uuid,
            kzg_proof: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ZiskResponse {
    pub proof: Option<String>,
    pub receipt: Option<String>, 
    pub input: Option<B256>,
    pub uuid: Option<String>,
}


pub struct ZiskProver;

impl Prover for ZiskProver {
    async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("no block run after pacaya fork")
    }

    async fn cancel(&self, key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        id_store.remove_id(key).await?;
        Ok(())
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param = ZiskParam::deserialize(config.get("zisk").unwrap()).unwrap();
        
        // Debug: Log aggregation input details
        info!("ZisK aggregation request: {} proofs", input.proofs.len());
        for (i, proof) in input.proofs.iter().enumerate() {
            info!("  Proof {}: input={:?}, quote_len={}, uuid={:?}, proof_len={}", 
                i,
                proof.input,
                proof.quote.as_ref().map(|q| q.len()).unwrap_or(0),
                proof.uuid,
                proof.proof.as_ref().map(|p| p.len()).unwrap_or(0)
            );
        }
        
        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .enumerate()
            .map(|(i, proof)| {
                proof.input.ok_or_else(|| {
                    ProverError::GuestError(format!(
                        "Proof {} input is None. Proof details: quote={:?}, uuid={:?}, proof_len={}", 
                        i,
                        proof.quote.as_ref().map(|q| format!("present, size:{}", q.len())),
                        proof.uuid,
                        proof.proof.as_ref().map(|p| p.len()).unwrap_or(0)
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
            
        // Generate image ID from Zisk aggregation ELF hash
        let elf_hash = keccak(AGGREGATION_ELF);
        let mut image_id = [0u32; 8];
        for (i, chunk) in elf_hash.chunks(4).enumerate().take(8) {
            image_id[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        let zisk_input = ZkAggregationGuestInput {
            image_id,
            block_inputs: block_inputs.clone(),
        };
        
        // Generate deterministic request ID based on proof inputs
        let mut hasher = DefaultHasher::new();
        for proof in &input.proofs {
            // Hash the proof input to create deterministic ID
            if let Some(input_hash) = proof.input {
                input_hash.hash(&mut hasher);
            }
        }
        let hash_value = hasher.finish();
        let request_id = format!("aggregation_{}_{}", input.proofs.len(), hash_value);
        let build_dir = format!("provers/zisk/build/{}", request_id);
        
        info!(
            "Zisk aggregate: {} proofs with request_id: {}",
            input.proofs.len(), request_id
        );


        // Default: Skip for performance
        // Optional: Enable for extra security validation
        if param.host_verification.unwrap_or(false) {
            info!("🐌 HOST VERIFICATION ENABLED");
            
            for (i, proof) in input.proofs.iter().enumerate() {
                if let Some(proof_data) = &proof.quote {
                    info!("Verifying proof {} using ZisK's native verification", i);
                    
                    // Write proof data to temporary file for verification
                    let temp_proof_path = format!("{}/temp_proof_{}.bin", build_dir, i);
                    std::fs::write(&temp_proof_path, proof_data.as_bytes())
                        .map_err(|e| ProverError::GuestError(format!("Failed to write temp proof: {e}")))?;
                    
                    // Verify proof using ZisK's native verification
                    let verify_output = Command::new("cargo-zisk")
                        .args(["verify", "-p", &temp_proof_path])
                        .output()
                        .map_err(|e| ProverError::GuestError(format!("ZisK proof verification failed: {e}")))?;
                    
                    if !verify_output.status.success() {
                        let error_msg = String::from_utf8_lossy(&verify_output.stderr);
                        return Err(ProverError::GuestError(format!(
                            "Proof {} verification failed: {}", i, error_msg
                        )));
                    }
                    
                    info!("Proof {} verified successfully using ZisK", i);
                    
                    // Clean up temporary proof file
                    let _ = std::fs::remove_file(&temp_proof_path);
                }
            }
        } else {
            info!("To enable host verification, set 'host_verification: true' in zisk config");
        }

        // Create input file for Zisk - use unique build directory
        let input_data = bincode::serialize(&zisk_input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
        
        // Ensure unique build directory exists
        std::fs::create_dir_all(&build_dir)
            .map_err(|e| ProverError::GuestError(format!("Failed to create build directory: {e}")))?;
        
        let input_file_path = format!("{}/input.bin", build_dir);
        std::fs::write(&input_file_path, input_data)
            .map_err(|e| ProverError::GuestError(format!("Failed to write input file: {e}")))?;



        // Use the permanent ELF file instead of temporary copy
        let temp_elf_path = "provers/zisk/guest/elf/zisk-aggregation";

        let prove_result = {
                // Ensure ROM setup is done (only if not already completed)
                ensure_rom_setup(&temp_elf_path).await?;
                
                // Generate proof with optional MPI concurrency
                generate_proof_with_mpi(
                    &temp_elf_path,
                    &input_file_path,
                    &format!("{}/proof", build_dir),
                    param.concurrent_processes,
                    param.threads_per_process,
                )?;

                // Read proof file from Zisk's expected location
                let proof_file_path = format!("{}/proof/vadcop_final_proof.bin", build_dir);
                let proof_data = std::fs::read(&proof_file_path)
                    .map_err(|e| ProverError::GuestError(format!("Failed to read proof file {}: {}", proof_file_path, e)))?;
                
                let proof_hex = hex::encode(&proof_data);
                
                if param.verify {  // Additional verification if requested
                    let time = Measurement::start("verify", false);
                    
                    // Use the proof file for verification
                    let proof_file_path = format!("{}/proof/vadcop_final_proof.bin", build_dir);
                    
                    let output = Command::new("cargo-zisk")
                        .args(["verify", "-p", &proof_file_path])
                        .output()
                        .map_err(|e| ProverError::GuestError(format!("Zisk verify failed: {e}")))?;
                    
                    if !output.status.success() {
                        return Err(ProverError::GuestError(format!(
                            "Zisk verification failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        )));
                    }
                    
                    time.stop_with("==> Zisk aggregation verification complete");
                }

                // Calculate proper input hash using aggregation_output function
                let program_id = B256::from(words_to_bytes_le(&image_id));
                let aggregation_pi = aggregation_output(program_id, block_inputs.clone());
                let input_hash = keccak(&aggregation_pi);
                
                // Create response before cleanup
                let response = ZiskResponse {
                    proof: Some(format!("0x{}", proof_hex)),
                    receipt: Some("zisk_aggregation_receipt".to_string()),
                    input: Some(B256::from_slice(&input_hash)),
                    uuid: Some("zisk_aggregation_uuid".to_string()),
                };

                // Clean up build directory immediately after successful aggregation
                if let Err(e) = std::fs::remove_dir_all(&build_dir) {
                    info!("Warning: Failed to clean up aggregation build directory {}: {}", build_dir, e);
                } else {
                    info!("Cleaned up aggregation build directory: {}", build_dir);
                }

                response
        };

        Ok(prove_result.into())
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param = ZiskParam::deserialize(config.get("zisk").unwrap()).unwrap();

        // Generate unique request ID for this batch
        let request_id = generate_request_id(input.taiko.batch_id, false, None);
        let build_dir = format!("provers/zisk/build/{}", request_id);

        info!(
            "Zisk Prover: batch {} with output hash: {} (request_id: {})",
            input.taiko.batch_id,
            output.hash,
            request_id
        );

        // Use the full GuestBatchInput like SP1/RISC0 - contains all blockchain execution data
        let input_data = bincode::serialize(&input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize GuestBatchInput: {e}")))?;
        
        // Create input file for Zisk - use unique build directory
        // Ensure unique build directory exists
        std::fs::create_dir_all(&build_dir)
            .map_err(|e| ProverError::GuestError(format!("Failed to create build directory: {e}")))?;
        
        let input_file_path = format!("{}/input.bin", build_dir);
        std::fs::write(&input_file_path, input_data)
            .map_err(|e| ProverError::GuestError(format!("Failed to write input file: {e}")))?;

        // Use the permanent ELF file instead of temporary copy
        let temp_batch_elf_path = "provers/zisk/guest/elf/zisk-batch";

        // Verify Zisk constraints before proof generation
        // verify_zisk_constraints(&temp_batch_elf_path, &input_file_path)?;

        let prove_result = {
                // Ensure ROM setup is done (only if not already completed)
                ensure_rom_setup(&temp_batch_elf_path).await?;
                
                // Generate proof with optional MPI concurrency
                generate_proof_with_mpi(
                    &temp_batch_elf_path,
                    &input_file_path,
                    &format!("{}/proof", build_dir),
                    param.concurrent_processes,
                    param.threads_per_process,
                )?;

                // Read proof file from Zisk's expected location
                let proof_file_path = format!("{}/proof/vadcop_final_proof.bin", build_dir);
                let proof_data = std::fs::read(&proof_file_path)
                    .map_err(|e| ProverError::GuestError(format!("Failed to read proof file {}: {}", proof_file_path, e)))?;
                
                let proof_hex = hex::encode(&proof_data);
                
                if param.verify {
                    let time = Measurement::start("verify", false);
                    
                    // Use the proof file for verification
                    let proof_file_path = format!("{}/proof/vadcop_final_proof.bin", build_dir);
                    
                    let verify_output = Command::new("cargo-zisk")
                        .args(["verify", "-p", &proof_file_path])
                        .output()
                        .map_err(|e| ProverError::GuestError(format!("Zisk verify failed: {e}")))?;
                    
                    if !verify_output.status.success() {
                        return Err(ProverError::GuestError(format!(
                            "Zisk verification failed: {}",
                            String::from_utf8_lossy(&verify_output.stderr)
                        )));
                    }
                    
                    time.stop_with("==> Zisk batch verification complete");
                }

                // Create response before cleanup
                let response = ZiskResponse {
                    proof: Some(format!("0x{}", proof_hex)),
                    receipt: Some("zisk_batch_receipt".to_string()),
                    input: Some(output.hash),
                    uuid: Some("zisk_batch_uuid".to_string()),
                };

                // Clean up build directory immediately after successful proof generation
                if let Err(e) = std::fs::remove_dir_all(&build_dir) {
                    info!("Warning: Failed to clean up build directory {}: {}", build_dir, e);
                } else {
                    info!("Cleaned up build directory: {}", build_dir);
                }

                response
        };

        info!(
            "Zisk Prover: batch {} completed!",
            input.taiko.batch_id
        );
        
        Ok(prove_result.into())
    }
}

/// Generate a unique request ID based on batch information
fn generate_request_id(batch_id: u64, is_aggregation: bool, batch_ids: Option<&[u64]>) -> String {
    if is_aggregation {
        if let Some(ids) = batch_ids {
            let mut sorted_ids = ids.to_vec();
            sorted_ids.sort();
            let ids_str = sorted_ids.iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join("_");
            format!("aggregation_{}", ids_str)
        } else {
            format!("aggregation_{}", batch_id)
        }
    } else {
        format!("batch_{}", batch_id)
    }
}

/// Run ROM setup only if it hasn't been done for this ELF yet
/// Multiple concurrent requests coordinate properly - only one does setup, others wait
async fn ensure_rom_setup(elf_path: &str) -> Result<(), ProverError> {
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
            // Another request is doing ROM setup, wait for it
            info!("ROM setup already in progress for ELF: {}, waiting...", elf_path);
            existing_notify.clone()
        } else {
            let notify = Arc::new(Notify::new());
            in_progress.insert(elf_path.to_string(), notify.clone());
            
            info!("Starting ROM setup for ELF: {} (first request)", elf_path);
            
            // Release the lock before running the blocking ROM setup command
            drop(in_progress);
            
            // Run the actual ROM setup command (blocking)
            let rom_result = tokio::task::spawn_blocking({
                let elf_path = elf_path.to_string();
                move || {
                    Command::new("cargo-zisk")
                        .args(["rom-setup", "-e", &elf_path])
                        .output()
                        .map_err(|e| ProverError::GuestError(format!("Zisk ROM setup failed: {e}")))
                }
            }).await;
            
            let rom_output = match rom_result {
                Ok(result) => result?,
                Err(e) => return Err(ProverError::GuestError(format!("ROM setup task failed: {e}"))),
            };
            
            if !rom_output.status.success() {
                // ROM setup failed, clean up in_progress state
                coordinator.in_progress.lock().await.remove(elf_path);
                notify.notify_waiters(); // Wake up waiting requests so they can see the failure
                
                return Err(ProverError::GuestError(format!(
                    "Zisk ROM setup failed: {}",
                    String::from_utf8_lossy(&rom_output.stderr)
                )));
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
            Err(ProverError::GuestError(format!(
                "ROM setup failed for ELF: {}", elf_path
            )))
        }
    }
}

/// Check if proof already exists and return it
fn read_existing_proof(build_dir: &str) -> Result<ZiskResponse, ProverError> {
    let proof_file_path = format!("{}/proof/vadcop_final_proof.bin", build_dir);
    let metadata_file = format!("{}/metadata.json", build_dir);
    
    if !Path::new(&proof_file_path).exists() {
        return Err(ProverError::GuestError("Proof file not found".to_string()));
    }
    
    let proof_data = std::fs::read(&proof_file_path)
        .map_err(|e| ProverError::GuestError(format!("Failed to read existing proof: {}", e)))?;
    
    let proof_hex = hex::encode(&proof_data);
    
    let (receipt, input, uuid) = if Path::new(&metadata_file).exists() {
        if let Ok(metadata_str) = std::fs::read_to_string(&metadata_file) {
            if let Ok(metadata_json) = serde_json::from_str::<serde_json::Value>(&metadata_str) {
                let receipt = metadata_json.get("receipt")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string());
                let input = metadata_json.get("input")
                    .and_then(|i| i.as_str())
                    .and_then(|s| s.parse::<B256>().ok());
                let uuid = metadata_json.get("uuid")
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string());
                (receipt, input, uuid)
            } else {
                (Some(metadata_str), None, None)
            }
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };
    
    Ok(ZiskResponse {
        proof: Some(proof_hex),
        receipt,
        input,
        uuid,
    })
}

/// Generate proof using MPI for concurrent execution if configured
fn generate_proof_with_mpi(
    elf_path: &str,
    input_path: &str, 
    output_dir: &str,
    concurrent_processes: Option<u32>,
    threads_per_process: Option<u32>,
) -> Result<(), ProverError> {
    let output = if let (Some(processes), Some(threads)) = (concurrent_processes, threads_per_process) {
        // Use MPI for concurrent proof generation
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
                "-a", "-y"
            ])
            .output()
            .map_err(|e| ProverError::GuestError(format!("Zisk MPI prove failed: {e}")))?
    } else {
        Command::new("cargo-zisk")
            .args([
                "prove",
                "-e", elf_path,
                "-i", input_path,
                "-o", output_dir,
                "-a", "-y"
            ])
            .output()
            .map_err(|e| ProverError::GuestError(format!("Zisk prove failed: {e}")))?
    };
    
    if !output.status.success() {
        return Err(ProverError::GuestError(format!(
            "Zisk prove failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    
    Ok(())
}

/// Verify Zisk constraints using the official cargo-zisk verify-constraints command
fn verify_zisk_constraints(elf_path: &str, input_path: &str) -> Result<(), ProverError> {
    info!("🔍 Verifying Zisk constraints for GuestBatchInput using cargo-zisk");
    
    // Get Zisk binary paths
    let witness_lib_path = std::env::var("HOME")
        .map(|home| format!("{}/.zisk/bin/libzisk_witness.so", home))
        .unwrap_or_else(|_| "$HOME/.zisk/bin/libzisk_witness.so".to_string());
    
    let proving_key_path = std::env::var("HOME")
        .map(|home| format!("{}/.zisk/provingKey", home))
        .unwrap_or_else(|_| "$HOME/.zisk/provingKey".to_string());
    
    info!("📋 Using paths:");
    info!("  - ELF: {}", elf_path);
    info!("  - Input: {}", input_path);
    info!("  - Witness lib: {}", witness_lib_path);
    info!("  - Proving key: {}", proving_key_path);
    
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
        .map_err(|e| ProverError::GuestError(format!("Failed to run cargo-zisk verify-constraints: {e}")))?;
    
    // Check if verification succeeded
    if output.status.success() {
        info!("✅ Zisk constraints verification PASSED");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            info!("📝 Verification output:\n{}", stdout);
        }
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        info!("❌ Zisk constraints verification FAILED");
        if !stdout.is_empty() {
            info!("📝 Verification stdout:\n{}", stdout);
        }
        if !stderr.is_empty() {
            info!("🔥 Verification stderr:\n{}", stderr);
        }
        
        Err(ProverError::GuestError(format!(
            "Zisk constraints verification failed. This indicates the GuestBatchInput is too large or complex for Zisk to handle. Error: {}",
            stderr.trim()
        )))
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deserialize_zisk_param() {
        let json = json!(
            {
                "prover": "local",
                "verify": true
            }
        );
        let param = ZiskParam {
            prover: Some(ProverMode::Local),
            verify: true,
        };
        let serialized = serde_json::to_value(param).unwrap();
        assert_eq!(json, serialized);

        let deserialized: ZiskParam = serde_json::from_value(serialized).unwrap();
        println!("{json:?} {deserialized:?}");
    }
}