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
use std::collections::{hash_map::DefaultHasher, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use tracing::info;

pub const BATCH_ELF: &[u8] = include_bytes!("../../guest/elf/zisk-batch");
pub const AGGREGATION_ELF: &[u8] = include_bytes!("../../guest/elf/zisk-aggregation");

// Global state to track which ELFs have had ROM setup completed
static ROM_SETUP_COMPLETED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

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
    /// Warning: This creates O(N) verification cost vs SP1/RISC0's O(1)
    /// Set to true only for extra security validation during development
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
        
        // Use the standard ZkAggregationGuestInput format for consistency with SP1/RISC0
        
        let zisk_input = ZkAggregationGuestInput {
            image_id,
            block_inputs: block_inputs.clone(),
        };
        
        // Generate deterministic request ID based on proof inputs
        // Since batch IDs are not directly available in proofs, we use a hash of all proof inputs
        // This ensures the same set of proofs always generates the same directory name
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

        // Check if proof already exists (for polling requests)
        if let Ok(existing_proof) = read_existing_proof(&build_dir) {
            info!("Returning existing aggregation proof for request {}", request_id);
            return Ok(existing_proof.into());
        }

        // 🔥 CONFIGURABLE HOST VERIFICATION
        // Default: Skip for performance
        // Optional: Enable for extra security validation
        if param.host_verification.unwrap_or(false) {
            info!("🐌 HOST VERIFICATION ENABLED: This creates O(N) cost vs SP1/RISC0's O(1)");
            
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
                    
                    info!("✅ Proof {} verified successfully using ZisK", i);
                    
                    // Clean up temporary proof file
                    let _ = std::fs::remove_file(&temp_proof_path);
                }
            }
        } else {
            info!("⚡ PERFORMANCE MODE: Skipping host verification (matching SP1/RISC0 approach)");
            info!("Will rely on guest-side cryptographic validation in aggregation guest program");
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
                ensure_rom_setup(&temp_elf_path)?;
                
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
                
                ZiskResponse {
                    proof: Some(format!("0x{}", proof_hex)),
                    receipt: Some("zisk_aggregation_receipt".to_string()),
                    input: Some(B256::from_slice(&input_hash)),
                    uuid: Some("zisk_aggregation_uuid".to_string()),
                }
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

        // Check if proof already exists (for polling requests)
        if let Ok(existing_proof) = read_existing_proof(&build_dir) {
            info!("Returning existing batch proof for request {}", request_id);
            return Ok(existing_proof.into());
        }

        // Transform input to match Zisk guest program expectations
        use serde::{Deserialize, Serialize};
        
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct ZiskBatchInput {
            pub batch_id: u64,
            pub chain_id: u64,
            pub block_numbers: Vec<u64>,
            pub block_hashes: Vec<[u8; 32]>,
        }
        
        let zisk_input = ZiskBatchInput {
            batch_id: input.taiko.batch_id,
            chain_id: input.taiko.chain_spec.chain_id,
            // For now, just use basic data from the input
            block_numbers: input.inputs.iter().map(|inp| inp.block.header.number).collect(),
            // Use parent_hash as a placeholder for block hash  
            block_hashes: input.inputs.iter().map(|inp| inp.block.header.parent_hash.0).collect(),
        };
        
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
        let temp_batch_elf_path = "provers/zisk/guest/elf/zisk-batch";

        let prove_result = {
                // Ensure ROM setup is done (only if not already completed)
                ensure_rom_setup(&temp_batch_elf_path)?;
                
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

                ZiskResponse {
                    proof: Some(format!("0x{}", proof_hex)),
                    receipt: Some("zisk_batch_receipt".to_string()),
                    input: Some(output.hash),
                    uuid: Some("zisk_batch_uuid".to_string()),
                }
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
fn ensure_rom_setup(elf_path: &str) -> Result<(), ProverError> {
    let mut completed_elfs = ROM_SETUP_COMPLETED.lock().unwrap();
    
    // Check if ROM setup was already completed for this ELF
    if completed_elfs.contains(elf_path) {
        info!("ROM setup already completed for ELF: {}", elf_path);
        return Ok(());
    }
    
    info!("Running ROM setup for ELF: {} (first time)", elf_path);
    let rom_output = Command::new("cargo-zisk")
        .args(["rom-setup", "-e", elf_path])
        .output()
        .map_err(|e| ProverError::GuestError(format!("Zisk ROM setup failed: {e}")))?;
    
    if !rom_output.status.success() {
        return Err(ProverError::GuestError(format!(
            "Zisk ROM setup failed: {}",
            String::from_utf8_lossy(&rom_output.stderr)
        )));
    }
    
    // Mark this ELF as having completed ROM setup
    completed_elfs.insert(elf_path.to_string());
    info!("ROM setup completed successfully for {}", elf_path);
    Ok(())
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
    
    // Try to read metadata for additional info
    let metadata = if Path::new(&metadata_file).exists() {
        std::fs::read_to_string(&metadata_file).ok()
    } else {
        None
    };
    
    Ok(ZiskResponse {
        proof: Some(proof_hex),
        receipt: metadata,
        input: None,
        uuid: None,
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
        // Use standard single-process proof generation
        info!("Using standard single-process proof generation");
        
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