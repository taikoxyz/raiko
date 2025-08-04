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
use std::process::Command;
use tracing::info;

// Zisk ELF file paths - generated during build via cargo-zisk
const ZISK_BATCH_ELF: &str = "provers/zisk/guest/target/riscv64ima-zisk-zkvm-elf/release/zisk-batch";
const ZISK_AGGREGATION_ELF: &str = "provers/zisk/guest/target/riscv64ima-zisk-zkvm-elf/release/zisk-aggregation";

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZiskParam {
    pub prover: Option<ProverMode>,
    #[serde(default = "DEFAULT_TRUE")]
    pub verify: bool,
    pub execution_mode: Option<ExecutionMode>,
}

const DEFAULT_TRUE: fn() -> bool = || true;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProverMode {
    Local,
    Remote,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    /// Execute with Zisk emulator for testing
    Emulator,
    /// Full proof generation
    #[default]
    Prove,
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
        
        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .map(|proof| proof.input.unwrap())
            .collect::<Vec<_>>();
            
        // Generate image ID from Zisk aggregation ELF path hash
        let elf_hash = keccak(ZISK_AGGREGATION_ELF.as_bytes());
        let mut image_id = [0u32; 8];
        for (i, chunk) in elf_hash.chunks(4).enumerate().take(8) {
            image_id[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        
        let aggregation_input = ZkAggregationGuestInput {
            image_id,
            block_inputs: block_inputs.clone(),
        };
        
        info!(
            "Zisk aggregate: {} proofs with inputs: {:?}",
            input.proofs.len(),
            aggregation_input.block_inputs
        );

        // Create input file for Zisk
        let input_data = bincode::serialize(&aggregation_input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
        
        // Ensure target directory exists
        let target_dir = "provers/zisk/guest/target";
        std::fs::create_dir_all(target_dir)
            .map_err(|e| ProverError::GuestError(format!("Failed to create target directory: {e}")))?;
        
        let input_file_path = format!("{}/zisk_aggregation_input.bin", target_dir);
        std::fs::write(&input_file_path, input_data)
            .map_err(|e| ProverError::GuestError(format!("Failed to write input file: {e}")))?;

        let prove_result = match param.execution_mode.unwrap_or_default() {
            ExecutionMode::Emulator => {
                // Run with emulator for testing
                let output = Command::new("ziskemu")
                    .args(["-e", ZISK_AGGREGATION_ELF, "-i", &input_file_path])
                    .output()
                    .map_err(|e| ProverError::GuestError(format!("Zisk emulator failed: {e}")))?;
                
                if !output.status.success() {
                    return Err(ProverError::GuestError(format!(
                        "Zisk emulator failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )));
                }
                
                info!("Zisk emulator execution successful");
                ZiskResponse {
                    proof: Some("emulator_proof".to_string()),
                    receipt: None,
                    input: Some(B256::default()),
                    uuid: None,
                }
            }
            ExecutionMode::Prove => {
                // First ensure ROM setup is done
                let rom_output = Command::new("cargo-zisk")
                    .args(["rom-setup", "-e", ZISK_AGGREGATION_ELF])
                    .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
                    .output()
                    .map_err(|e| ProverError::GuestError(format!("Zisk ROM setup failed: {e}")))?;
                
                if !rom_output.status.success() {
                    return Err(ProverError::GuestError(format!(
                        "Zisk ROM setup failed: {}",
                        String::from_utf8_lossy(&rom_output.stderr)
                    )));
                }
                
                info!("ROM setup completed successfully");
                
                // Generate proof with memory optimization flags
                let output = Command::new("cargo-zisk")
                    .args([
                        "prove", 
                        "-e", ZISK_AGGREGATION_ELF, 
                        "-i", &input_file_path, 
                        "-o", "aggregation_proof", 
                        "-a",                // aggregation mode
                        "--minimal-memory",  // Use minimal memory mode
                        "-u",                // Unlock memory mapping
                        "-p", "23200",       // Use different port to avoid conflicts
                        "-y"                 // verify after proving
                    ])
                    .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
                    .output()
                    .map_err(|e| ProverError::GuestError(format!("Zisk prove failed: {e}")))?;
                
                if !output.status.success() {
                    return Err(ProverError::GuestError(format!(
                        "Zisk prove failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )));
                }

                // Read proof file
                let proof_data = std::fs::read("aggregation_proof/vadcop_final_proof.bin")
                    .map_err(|e| ProverError::GuestError(format!("Failed to read proof: {e}")))?;
                
                let proof_hex = hex::encode(&proof_data);
                
                if param.verify {  // Additional verification if requested
                    let time = Measurement::start("verify", false);
                    
                    let output = Command::new("cargo-zisk")
                        .args(["verify", "-p", "aggregation_proof/vadcop_final_proof.bin"])
                        .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
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

        info!(
            "Zisk Prover: batch {} with output hash: {}",
            input.taiko.batch_id,
            output.hash
        );

        // Transform input to match Zisk guest program expectations
        use serde::{Deserialize, Serialize};
        
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct ZiskBatchInput {
            pub batch_id: u64,
            pub chain_id: u64,
            pub block_numbers: Vec<u64>,
            pub block_hashes: Vec<[u8; 32]>,
            pub use_emulator_mode: Option<bool>,
        }
        
        let execution_mode = param.execution_mode.clone().unwrap_or_default();
        let zisk_input = ZiskBatchInput {
            batch_id: input.taiko.batch_id,
            chain_id: input.taiko.chain_spec.chain_id,
            // For now, just use basic data from the input
            block_numbers: input.inputs.iter().map(|inp| inp.block.header.number).collect(),
            // Use parent_hash as a placeholder for block hash  
            block_hashes: input.inputs.iter().map(|inp| inp.block.header.parent_hash.0).collect(),
            // Pass execution mode to guest program
            use_emulator_mode: Some(matches!(execution_mode, ExecutionMode::Emulator)),
        };
        
        // Create input file for Zisk
        let input_data = bincode::serialize(&zisk_input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
        
        // Ensure target directory exists
        let target_dir = "provers/zisk/guest/target";
        std::fs::create_dir_all(target_dir)
            .map_err(|e| ProverError::GuestError(format!("Failed to create target directory: {e}")))?;
        
        let input_file_path = format!("{}/zisk_batch_input.bin", target_dir);
        std::fs::write(&input_file_path, input_data)
            .map_err(|e| ProverError::GuestError(format!("Failed to write input file: {e}")))?;

        let prove_result = match execution_mode {
            ExecutionMode::Emulator => {
                // Run with emulator for testing
                let cmd_output = Command::new("ziskemu")
                    .args(["-e", ZISK_BATCH_ELF, "-i", &input_file_path])
                    .output()
                    .map_err(|e| ProverError::GuestError(format!("Zisk emulator failed: {e}")))?;
                
                if !cmd_output.status.success() {
                    return Err(ProverError::GuestError(format!(
                        "Zisk emulator failed: {}",
                        String::from_utf8_lossy(&cmd_output.stderr)
                    )));
                }
                
                info!("Zisk emulator execution successful");
                ZiskResponse {
                    proof: Some("emulator_proof".to_string()),
                    receipt: None,
                    input: Some(output.hash),
                    uuid: None,
                }
            }
            ExecutionMode::Prove => {
                // First ensure ROM setup is done
                let rom_output = Command::new("cargo-zisk")
                    .args(["rom-setup", "-e", ZISK_BATCH_ELF])
                    .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
                    .output()
                    .map_err(|e| ProverError::GuestError(format!("Zisk ROM setup failed: {e}")))?;
                
                if !rom_output.status.success() {
                    return Err(ProverError::GuestError(format!(
                        "Zisk ROM setup failed: {}",
                        String::from_utf8_lossy(&rom_output.stderr)
                    )));
                }
                
                info!("ROM setup completed successfully");
                
                // Generate proof with memory optimization flags
                let prove_output = Command::new("cargo-zisk")
                    .args([
                        "prove", 
                        "-e", ZISK_BATCH_ELF, 
                        "-i", &input_file_path, 
                        "-o", "batch_proof",
                        "--minimal-memory",  // Use minimal memory mode
                        "-u",                // Unlock memory mapping
                        "-p", "23200",       // Use different port to avoid conflicts
                        "-y"                 // verify after proving
                    ])
                    .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
                    .output()
                    .map_err(|e| ProverError::GuestError(format!("Zisk prove failed: {e}")))?;
                
                if !prove_output.status.success() {
                    return Err(ProverError::GuestError(format!(
                        "Zisk prove failed: {}",
                        String::from_utf8_lossy(&prove_output.stderr)
                    )));
                }

                // Read proof file
                let proof_data = std::fs::read("batch_proof/vadcop_final_proof.bin")
                    .map_err(|e| ProverError::GuestError(format!("Failed to read proof: {e}")))?;
                
                let proof_hex = hex::encode(&proof_data);
                
                if param.verify {
                    let time = Measurement::start("verify", false);
                    
                    let verify_output = Command::new("cargo-zisk")
                        .args(["verify", "-p", "batch_proof/vadcop_final_proof.bin"])
                        .current_dir(std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
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
            }
        };

        info!(
            "Zisk Prover: batch {} completed! proof: {:?}",
            input.taiko.batch_id,
            prove_result.proof
        );
        
        Ok(prove_result.into())
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
                "verify": true,
                "execution_mode": "prove"
            }
        );
        let param = ZiskParam {
            prover: Some(ProverMode::Local),
            verify: true,
            execution_mode: Some(ExecutionMode::Prove),
        };
        let serialized = serde_json::to_value(param).unwrap();
        assert_eq!(json, serialized);

        let deserialized: ZiskParam = serde_json::from_value(serialized).unwrap();
        println!("{json:?} {deserialized:?}");
    }
}