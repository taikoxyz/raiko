pub mod boundless;
pub use boundless::Risc0BoundlessProver;

pub mod methods;

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, ValueEnum)]
enum ProofType {
    Batch,
    Agg,
}

#[derive(Debug, Parser)]
#[command(name = "risc0-boundless-agent")]
#[command(about = "RISC0 Boundless proof generation agent")]
#[command(version)]
struct Args {
    /// Input file path (required)
    #[arg(short, long)]
    input: PathBuf,

    /// Output file path (required)
    #[arg(short, long)]
    output: PathBuf,

    /// Proof type
    #[arg(short, long, value_enum, default_value_t = ProofType::Batch)]
    proof_type: ProofType,

    /// ELF file path (optional, uses default if not specified)
    #[arg(short, long)]
    elf: Option<PathBuf>,

    /// Configuration file path (optional)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Debug, Clone)]
struct ProofRequest {
    input: Vec<u8>,
    output: Option<Vec<u8>>,
    proof_type: ProofType,
    elf_path: Option<PathBuf>,
    config: serde_json::Value,
    verbose: bool,
    output_file: PathBuf,
}

impl ProofRequest {
    fn from_args(args: Args) -> Result<Self, Box<dyn std::error::Error>> {
        // Read input file
        let input = std::fs::read(&args.input)
            .map_err(|e| format!("Failed to read input file {}: {}", args.input.display(), e))?;

        // Read output file (now required)
        // let output = std::fs::read(&args.output)
        //     .map_err(|e| format!("Failed to read output file {}: {}", args.output.display(), e))?;

        // Read config file if provided, otherwise use default
        let config = if let Some(config_path) = &args.config {
            let config_content = std::fs::read_to_string(config_path).map_err(|e| {
                format!(
                    "Failed to read config file {}: {}",
                    config_path.display(),
                    e
                )
            })?;
            serde_json::from_str(&config_content)
                .map_err(|e| format!("Failed to parse config file: {}", e))?
        } else {
            serde_json::Value::default()
        };

        Ok(ProofRequest {
            input,
            output: None,
            proof_type: args.proof_type,
            elf_path: args.elf,
            config,
            verbose: args.verbose,
            output_file: args.output,
        })
    }
}

#[derive(Debug)]
struct ProofResponse {
    proof_data: Vec<u8>,
    proof_type: ProofType,
}

impl ProofResponse {
    fn new(proof_data: Vec<u8>, proof_type: ProofType) -> Self {
        Self {
            proof_data,
            proof_type,
        }
    }

    fn write_to_file(&self, file_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(file_path, &self.proof_data).map_err(|e| {
            format!(
                "Failed to write proof to file {}: {}",
                file_path.display(),
                e
            )
            .into()
        })
    }
}

async fn generate_proof(
    request: ProofRequest,
) -> Result<ProofResponse, Box<dyn std::error::Error>> {
    tracing::info!("Initializing prover...");
    
    // Add timeout for prover initialization
    let prover = tokio::time::timeout(
        std::time::Duration::from_secs(500), // 5 minutes timeout
        Risc0BoundlessProver::init_prover()
    )
    .await
    .map_err(|_| "Prover initialization timed out after 5 minutes".to_string())?
    .map_err(|e| format!("Failed to initialize prover: {}", e))?;

    // Use empty output if not provided
    let output_data = request.output.unwrap_or_default();
    tracing::info!("Running proof...");
    
    // Add timeout for proof generation
    let proof_data = tokio::time::timeout(
        std::time::Duration::from_secs(3600), // 10 minutes timeout
        async {
            match request.proof_type {
                ProofType::Batch => prover
                    .batch_run(request.input, &output_data, &request.config)
                    .await
                    .map_err(|e| format!("Failed to run batch proof: {}", e)),
                ProofType::Agg => prover
                    .aggregate(request.input, &output_data, &request.config)
                    .await
                    .map_err(|e| format!("Failed to run aggregation proof: {}", e)),
            }
        }
    )
    .await
    .map_err(|_| "Proof generation timed out after 10 minutes".to_string())??;

    Ok(ProofResponse::new(proof_data, request.proof_type))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args = Args::parse();

    if args.verbose {
        println!("Starting RISC0 Boundless proof generation...");
        println!("Input file: {}", args.input.display());
        println!("Output file: {}", args.output.display());
        println!("Proof type: {:?}", args.proof_type);
        if let Some(ref elf) = args.elf {
            println!("ELF file: {}", elf.display());
        }
    }

    // Parse the proof request
    let request = ProofRequest::from_args(args)?;

    if request.verbose {
        println!("Input data size: {} bytes", request.input.len());
        if let Some(ref output) = request.output {
            println!("Output data size: {} bytes", output.len());
        }
    }

    // Generate the proof
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create tokio runtime: {}", e))?;

    let response = runtime.block_on(generate_proof(request.clone()))?;

    if request.verbose {
        println!("Generated proof size: {} bytes", response.proof_data.len());
        println!("Writing proof to file: {}", request.output_file.display());
    }

    // Write the proof to file
    response.write_to_file(&request.output_file)?;

    if request.verbose {
        println!(
            "Proof successfully written to: {}",
            request.output_file.display()
        );
    }

    Ok(())
}
