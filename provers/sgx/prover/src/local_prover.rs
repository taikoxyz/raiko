#![cfg(feature = "enable")]

use std::{
    env,
    fs::{copy, create_dir_all, remove_file},
    io::Seek,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Output, Stdio},
    str::{self, FromStr},
};

use duct::{cmd, Expression};
use once_cell::sync::Lazy;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, RawAggregationGuestInput, RawProof, ShastaAggregationGuestInput,
        ShastaRawAggregationGuestInput,
    },
    primitives::B256,
    proof_type::ProofType,
    prover::{
        IdStore, IdWrite, Proof, ProofCarryData, ProofKey, Prover, ProverConfig, ProverError,
        ProverResult,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{process::Command, sync::OnceCell};
use tracing::error;

pub use crate::sgx_register_utils::{
    get_instance_id, register_sgx_instance, remove_instance_id, set_instance_id, ForkRegisterId,
};

pub const PRIV_KEY_FILENAME: &str = "priv.key";
pub const PRIV_KEY_FILENAME_GAIKO: &str = "priv.gaiko.key";

pub fn get_priv_key_filename(proof_type: ProofType) -> &'static str {
    match proof_type {
        ProofType::Sgx => PRIV_KEY_FILENAME,
        ProofType::SgxGeth => PRIV_KEY_FILENAME_GAIKO,
        _ => panic!("Invalid proof type for SGX prover"),
    }
}

use crate::{SgxParam, SgxResponse};

pub const ELF_NAME: &str = "sgx-guest";
pub const GAIKO_ELF_NAME: &str = "gaiko";
#[cfg(feature = "docker_build")]
pub const CONFIG: &str = "../provers/sgx/config";
#[cfg(not(feature = "docker_build"))]
pub const CONFIG: &str = "../../provers/sgx/config";
static GRAMINE_MANIFEST_TEMPLATE: Lazy<OnceCell<PathBuf>> = Lazy::new(OnceCell::new);
static PRIVATE_KEY: Lazy<OnceCell<PathBuf>> = Lazy::new(OnceCell::new);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalSgxProver {
    proof_type: ProofType,
}

impl LocalSgxProver {
    pub fn new(proof_type: ProofType) -> Self {
        Self { proof_type }
    }
}

impl Prover for LocalSgxProver {
    async fn run(
        &self,
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Support both SGX and the direct backend for testing
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        println!(
            "WARNING: running SGX in {} mode!",
            if direct_mode {
                "direct (a.k.a. simulation)"
            } else {
                "hardware"
            }
        );

        // The working directory
        let mut cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir.parent().unwrap().to_path_buf();
        }

        println!("Current directory: {cur_dir:?}\n");
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async {
                cur_dir
                    .join(CONFIG)
                    .join("sgx-guest.local.manifest.template")
            })
            .await;

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> Expression {
            let cmd = if direct_mode {
                cmd!("gramine-direct", ELF_NAME).dir(&cur_dir)
            } else if self.proof_type == ProofType::SgxGeth {
                cmd!("sudo", cur_dir.join(GAIKO_ELF_NAME))
            } else {
                cmd!("sudo", "gramine-sgx", ELF_NAME).dir(&cur_dir)
            };
            cmd.unchecked()
        };

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            setup(&cur_dir, direct_mode).await?;
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap2(
                cur_dir.clone().join("secrets"),
                gramine_cmd(),
                self.proof_type,
            )
            .await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input, &sgx_param)?;
            sgx_proof = prove(gramine_cmd(), input.clone(), instance_id, self.proof_type).await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Support both SGX and the direct backend for testing
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        if self.proof_type == ProofType::Sgx {
            println!(
                "WARNING: running SGX in {} mode!",
                if direct_mode {
                    "direct (a.k.a. simulation)"
                } else {
                    "hardware"
                }
            );
        }

        // The working directory
        let mut cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir.parent().unwrap().to_path_buf();
        }

        println!("Current directory: {cur_dir:?}\n");
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async {
                cur_dir
                    .join(CONFIG)
                    .join("sgx-guest.local.manifest.template")
            })
            .await;

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> StdCommand {
            if self.proof_type == ProofType::SgxGeth {
                let mut cmd = StdCommand::new("sudo");
                cmd.arg(cur_dir.join(GAIKO_ELF_NAME));
                return cmd;
            }
            let mut cmd = if direct_mode {
                StdCommand::new("gramine-direct")
            } else {
                let mut cmd = StdCommand::new("sudo");
                cmd.arg("gramine-sgx");
                cmd
            };
            cmd.current_dir(&cur_dir).arg(ELF_NAME);
            cmd
        };

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            setup(&cur_dir, direct_mode).await?;
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(
                cur_dir.clone().join("secrets"),
                gramine_cmd(),
                self.proof_type,
            )
            .await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            sgx_proof = aggregate(gramine_cmd(), input.clone(), self.proof_type).await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn cancel(&self, _proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Support both SGX and the direct backend for testing
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        if self.proof_type == ProofType::Sgx {
            println!(
                "WARNING: running SGX in {} mode!",
                if direct_mode {
                    "direct (a.k.a. simulation)"
                } else {
                    "hardware"
                }
            );
        }

        // The working directory
        let mut cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir.parent().unwrap().to_path_buf();
        }

        println!("Current directory: {cur_dir:?}\n");
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async {
                cur_dir
                    .join(CONFIG)
                    .join("sgx-guest.local.manifest.template")
            })
            .await;

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> Expression {
            let cmd = if direct_mode {
                cmd!("gramine-direct", ELF_NAME).dir(&cur_dir)
            } else if self.proof_type == ProofType::SgxGeth {
                cmd!("sudo", cur_dir.join(GAIKO_ELF_NAME))
            } else {
                cmd!("sudo", "gramine-sgx", ELF_NAME).dir(&cur_dir)
            };
            cmd.unchecked()
        };

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup && self.proof_type != ProofType::SgxGeth {
            setup(&cur_dir, direct_mode).await?;
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap2(
                cur_dir.clone().join("secrets"),
                gramine_cmd(),
                self.proof_type,
            )
            .await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input.inputs[0], &sgx_param)?;
            sgx_proof =
                batch_prove(gramine_cmd(), input.clone(), instance_id, self.proof_type).await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Support both SGX and the direct backend for testing
        let direct_mode = match env::var("SGX_DIRECT") {
            Ok(value) => value == "1",
            Err(_) => false,
        };

        if self.proof_type == ProofType::Sgx {
            println!(
                "WARNING: running SGX in {} mode!",
                if direct_mode {
                    "direct (a.k.a. simulation)"
                } else {
                    "hardware"
                }
            );
        }

        // The working directory
        let mut cur_dir = env::current_exe()
            .expect("Fail to get current directory")
            .parent()
            .unwrap()
            .to_path_buf();

        // When running in tests we might be in a child folder
        if cur_dir.ends_with("deps") {
            cur_dir = cur_dir.parent().unwrap().to_path_buf();
        }

        println!("Current directory: {cur_dir:?}\n");
        // Working paths
        PRIVATE_KEY
            .get_or_init(|| async { cur_dir.join("secrets").join(PRIV_KEY_FILENAME) })
            .await;
        GRAMINE_MANIFEST_TEMPLATE
            .get_or_init(|| async {
                cur_dir
                    .join(CONFIG)
                    .join("sgx-guest.local.manifest.template")
            })
            .await;

        // The gramine command (gramine or gramine-direct for testing in non-SGX environment)
        let gramine_cmd = || -> StdCommand {
            if self.proof_type == ProofType::SgxGeth {
                let mut cmd = StdCommand::new("sudo");
                cmd.arg(cur_dir.join(GAIKO_ELF_NAME));
                return cmd;
            }
            let mut cmd = if direct_mode {
                StdCommand::new("gramine-direct")
            } else {
                let mut cmd = StdCommand::new("sudo");
                cmd.arg("gramine-sgx");
                cmd
            };
            cmd.current_dir(&cur_dir).arg(ELF_NAME);
            cmd
        };

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            setup(&cur_dir, direct_mode).await?;
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(
                cur_dir.clone().join("secrets"),
                gramine_cmd(),
                self.proof_type,
            )
            .await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            sgx_proof = shasta_aggregate(gramine_cmd(), input.clone(), self.proof_type).await
        }

        sgx_proof.map(|r| r.into())
    }

    fn proof_type(&self) -> ProofType {
        self.proof_type
    }
}

async fn setup(cur_dir: &Path, direct_mode: bool) -> ProverResult<(), String> {
    // Create required directories
    let directories = ["secrets", "config"];
    for dir in directories {
        create_dir_all(cur_dir.join(dir)).unwrap();
    }
    if direct_mode {
        // Copy dummy files in direct mode
        let files = ["attestation_type", "quote", "user_report_data"];
        for file in files {
            copy(
                cur_dir.join(CONFIG).join("dummy_data").join(file),
                cur_dir.join(file),
            )
            .unwrap();
        }
    }

    // Generate the manifest
    let mut cmd = Command::new("gramine-manifest");
    let output = cmd
        .current_dir(cur_dir)
        .arg("-Dlog_level=error")
        .arg("-Darch_libdir=/lib/x86_64-linux-gnu/")
        .arg(format!(
            "-Ddirect_mode={}",
            if direct_mode { "1" } else { "0" }
        ))
        .arg(GRAMINE_MANIFEST_TEMPLATE.get().unwrap())
        .arg("sgx-guest.manifest")
        .output()
        .await
        .map_err(|e| handle_gramine_error("Could not generate manfifest", e))?;
    handle_output(&output, "SGX generate manifest")?;

    if !direct_mode {
        // Generate a private key
        let mut cmd = Command::new("gramine-sgx-gen-private-key");
        let output = cmd
            .current_dir(cur_dir)
            .arg("-f")
            .output()
            .await
            .map_err(|e| handle_gramine_error("Could not generate SGX private key", e))?;
        handle_output(&output, "SGX private key")?;

        // Sign the manifest
        let mut cmd = Command::new("gramine-sgx-sign");
        let output = cmd
            .current_dir(cur_dir)
            .arg("--manifest")
            .arg("sgx-guest.manifest")
            .arg("--output")
            .arg("sgx-guest.manifest.sgx")
            .output()
            .await
            .map_err(|e| handle_gramine_error("Could not sign manfifest", e))?;
        handle_output(&output, "SGX manifest sign")?;
    }

    Ok(())
}

pub async fn check_bootstrap(
    secret_dir: PathBuf,
    mut gramine_cmd: StdCommand,
    proof_type: ProofType,
) -> ProverResult<(), ProverError> {
    tokio::task::spawn_blocking(move || {
        // Check if the private key exists
        let path = secret_dir.join(get_priv_key_filename(proof_type));
        if !path.exists() {
            Err(ProverError::GuestError(
                "Private key does not exist".to_string(),
            ))
        } else {
            // Check if the private key is valid
            let output = gramine_cmd.arg("check").output().map_err(|e| {
                ProverError::GuestError(handle_gramine_error(
                    "Could not run SGX guest bootstrap",
                    e,
                ))
            })?;
            handle_output(&output, "SGX check bootstrap")?;
            Ok(())
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

pub async fn bootstrap2(
    secret_dir: PathBuf,
    gramine_cmd: Expression,
    proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    tokio::task::spawn_blocking(move || {
        // Bootstrap with new private key for signing proofs
        // First delete the private key if it already exists
        let path = secret_dir.join(get_priv_key_filename(proof_type));
        if path.exists() {
            if let Err(e) = remove_file(&path) {
                println!("Error deleting file: {e}");
            }
        }
        let output = gramine_cmd
            .before_spawn(|cmd| {
                cmd.arg("bootstrap");
                Ok(())
            })
            .stdout_capture()
            .run()
            .map_err(|e| handle_gramine_error("Could not run SGX guest bootstrap", e))?;
        handle_output(&output, "SGX bootstrap")?;

        Ok(parse_sgx_result(output.stdout)?)
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

pub async fn bootstrap(
    secret_dir: PathBuf,
    mut gramine_cmd: StdCommand,
    proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    tokio::task::spawn_blocking(move || {
        // Bootstrap with new private key for signing proofs
        // First delete the private key if it already exists
        let path = secret_dir.join(get_priv_key_filename(proof_type));
        if path.exists() {
            if let Err(e) = remove_file(&path) {
                println!("Error deleting file: {e}");
            }
        }
        let output = gramine_cmd
            .arg("bootstrap")
            .output()
            .map_err(|e| handle_gramine_error("Could not run SGX guest bootstrap", e))?;
        handle_output(&output, "SGX bootstrap")?;

        Ok(parse_sgx_result(output.stdout)?)
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn prove(
    mut gramine_cmd: Expression,
    input: GuestInput,
    instance_id: u64,
    proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    tokio::task::spawn_blocking(move || {
        gramine_cmd = gramine_cmd
            .before_spawn(move |cmd| {
                cmd.arg("one-shot")
                    .arg("--sgx-instance-id")
                    .arg(instance_id.to_string());
                Ok(())
            })
            .stdout_capture()
            .stderr_capture();

        if proof_type == ProofType::SgxGeth {
            let mut temp_file = tempfile::tempfile()?;
            serde_json::to_writer(&temp_file, &input)
                .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
            temp_file.seek(std::io::SeekFrom::Start(0)).unwrap();
            gramine_cmd = gramine_cmd.stdin_file(temp_file);
        } else {
            let bytes = bincode::serialize(&input)
                .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
            gramine_cmd = gramine_cmd.stdin_bytes(bytes);
        }

        let output_success = gramine_cmd.run();
        match output_success {
            Ok(output) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            Err(output_err) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn batch_prove(
    mut gramine_cmd: Expression,
    input: GuestBatchInput,
    instance_id: u64,
    proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    tokio::task::spawn_blocking(move || {
        gramine_cmd = gramine_cmd
            .before_spawn(move |cmd| {
                cmd.arg("one-batch-shot")
                    .arg("--sgx-instance-id")
                    .arg(instance_id.to_string());
                Ok(())
            })
            .stdout_capture()
            .stderr_capture();

        if proof_type == ProofType::SgxGeth {
            let mut temp_file = tempfile::tempfile()?;
            serde_json::to_writer(&temp_file, &input)
                .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
            temp_file.seek(std::io::SeekFrom::Start(0)).unwrap();
            gramine_cmd = gramine_cmd.stdin_file(temp_file);
        } else {
            let bytes = bincode::serialize(&input)
                .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
            gramine_cmd = gramine_cmd.stdin_bytes(bytes);
        }

        let output_success = gramine_cmd.run();
        match output_success {
            Ok(output) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            Err(output_err) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn aggregate(
    mut gramine_cmd: StdCommand,
    input: AggregationGuestInput,
    proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // Extract the useful parts of the proof here so the guest doesn't have to do it
    let raw_input = RawAggregationGuestInput {
        proofs: input
            .proofs
            .iter()
            .map(|proof| RawProof {
                input: proof.clone().input.unwrap(),
                proof: hex::decode(&proof.clone().proof.unwrap()[2..]).unwrap(),
            })
            .collect(),
    };
    // Extract the instance id from the first proof
    let instance_id = {
        let mut instance_id_bytes = [0u8; 4];
        instance_id_bytes[0..4].copy_from_slice(&raw_input.proofs[0].proof.clone()[0..4]);
        u32::from_be_bytes(instance_id_bytes)
    };

    tokio::task::spawn_blocking(move || {
        let mut child = gramine_cmd
            .arg("aggregate")
            .arg("--sgx-instance-id")
            .arg(instance_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not spawn gramine cmd: {e}"))?;
        let stdin = child.stdin.take().expect("Failed to open stdin");
        tokio::task::spawn_blocking(move || {
            let _ = if proof_type == ProofType::SgxGeth {
                serde_json::to_writer(stdin, &raw_input)
                    .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))
            } else {
                bincode::serialize_into(stdin, &raw_input)
                    .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))
            }
            .inspect_err(|e| {
                error!("Failed to serialize input: {e}");
            });
        });
        let output_success = child.wait_with_output();

        match output_success {
            Ok(output) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            Err(output_err) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn shasta_aggregate(
    mut gramine_cmd: StdCommand,
    input: ShastaAggregationGuestInput,
    proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // Extract the useful parts of the proof here so the guest doesn't have to do it
    let (proofs, proof_carry_data_vec): (Vec<_>, Vec<_>) = input
        .proofs
        .iter()
        .map(|proof| {
            (
                RawProof {
                    input: proof.input.clone().unwrap(),
                    proof: hex::decode(&proof.proof.clone().unwrap()[2..]).unwrap(),
                },
                {
                    let extra_data = proof.extra_data.clone().unwrap();
                    ProofCarryData {
                        chain_id: extra_data.chain_id,
                        verifier: extra_data.verifier,
                        transition_input: extra_data.transition_input,
                    }
                },
            )
        })
        .unzip();
    let raw_input = ShastaRawAggregationGuestInput {
        proofs,
        proof_carry_data_vec,
    };
    // Extract the instance id from the first proof
    let instance_id = {
        let mut instance_id_bytes = [0u8; 4];
        instance_id_bytes[0..4].copy_from_slice(&raw_input.proofs[0].proof.clone()[0..4]);
        u32::from_be_bytes(instance_id_bytes)
    };

    tokio::task::spawn_blocking(move || {
        let mut child = gramine_cmd
            .arg("shasta_aggregate")
            .arg("--sgx-instance-id")
            .arg(instance_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not spawn gramine cmd: {e}"))?;
        let stdin = child.stdin.take().expect("Failed to open stdin");
        tokio::task::spawn_blocking(move || {
            let _ = if proof_type == ProofType::SgxGeth {
                serde_json::to_writer(stdin, &raw_input)
                    .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))
            } else {
                bincode::serialize_into(stdin, &raw_input)
                    .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))
            }
            .inspect_err(|e| {
                error!("Failed to serialize input: {e}");
            });
        });
        let output_success = child.wait_with_output();

        match output_success {
            Ok(output) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            Err(output_err) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

fn parse_sgx_result(output: Vec<u8>) -> ProverResult<SgxResponse, String> {
    let mut json_value: Option<Value> = None;
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;

    for line in output.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
            json_value = Some(value);
            break;
        }
    }
    let extract_field = |field| {
        json_value
            .as_ref()
            .and_then(|json| json.get(field).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string()
    };

    Ok(SgxResponse {
        proof: extract_field("proof"),
        quote: extract_field("quote"),
        input: B256::from_str(&extract_field("input")).unwrap_or_default(),
    })
}

fn handle_gramine_error(context: &str, err: std::io::Error) -> String {
    if let std::io::ErrorKind::NotFound = err.kind() {
        format!("gramine could not be found, please install gramine first. ({err})")
    } else {
        format!("{context}: {err}")
    }
}

fn handle_output(output: &Output, name: &str) -> ProverResult<(), String> {
    println!(
        "{name} stderr: {}",
        str::from_utf8(&output.stderr).unwrap_or_default()
    );
    println!(
        "{name} stdout: {}",
        str::from_utf8(&output.stdout).unwrap_or_default()
    );
    if !output.status.success() {
        return Err(format!(
            "{name} encountered an error ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(())
}

pub fn get_instance_id_from_params(input: &GuestInput, sgx_param: &SgxParam) -> ProverResult<u64> {
    let spec_id = input
        .chain_spec
        .active_fork(input.block.number, input.block.timestamp)
        .map_err(|e| ProverError::GuestError(e.to_string()))?;
    sgx_param
        .instance_ids
        .get(&spec_id)
        .cloned()
        .ok_or_else(|| {
            ProverError::GuestError(format!("No instance id found for spec id: {:?}", spec_id))
        })
}
