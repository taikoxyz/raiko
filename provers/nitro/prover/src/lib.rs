use std::{
    env,
    fs::{copy, create_dir_all, remove_file},
    path::{Path, PathBuf},
    process::{Command as StdCommand, Output, Stdio},
    str::{self, FromStr},
    sync::{Arc, Mutex},
};

use nitro_common::Command as NitroCommand;
use nitro_host::HostConnection;
use once_cell::sync::{Lazy, OnceCell};
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestInput, GuestOutput,
        RawAggregationGuestInput, RawProof,
    },
    primitives::B256,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::serde_as;

pub const PRIV_KEY_FILENAME: &str = "priv.key";

pub mod nitro_host;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NitroParam {
    pub setup: bool,
    pub instance_id: u64,
    pub bootstrap: bool,
    pub prove: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NitroResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
    pub input: B256,
}

impl From<NitroResponse> for Proof {
    fn from(value: NitroResponse) -> Self {
        Self {
            proof: Some(value.proof),
            input: Some(value.input),
            quote: Some(value.quote),
            uuid: None,
            kzg_proof: None,
        }
    }
}

const VMADDR_CID_ANY: u32 = 0xFFFFFFFF;
const VMADDR_PORT: u32 = 5000;
const LOG_PORT: u32 = 5001;
const BUFFER_SIZE: usize = 1024;

#[derive(Clone)]
pub struct SharedHostConnection {
    inner: Arc<Mutex<HostConnection>>,
}

impl SharedHostConnection {
    pub fn instance() -> Self {
        static INSTANCE: OnceCell<Arc<Mutex<HostConnection>>> = OnceCell::new();

        let inner = INSTANCE.get_or_init(|| {
            Arc::new(Mutex::new(
                HostConnection::listen(VMADDR_CID_ANY, VMADDR_PORT).unwrap(),
            ))
        });

        Self {
            inner: Arc::clone(inner),
        }
    }

    pub fn send_command(&self, command: NitroCommand) -> Result<nitro_common::Response, String> {
        let mut conn = self.inner.lock().unwrap();
        conn.send_command(command).map_err(|e| e.to_string())
    }
}

pub struct NitroProver;

impl Prover for NitroProver {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let nitro_param = NitroParam::deserialize(config.get("nitro").unwrap()).unwrap();

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

        // Setup: run this once while setting up your SGX instance
        if nitro_param.setup {
            setup(&cur_dir).await?;
        }

        let conn: SharedHostConnection = SharedHostConnection::instance();
        let mut nitro_proof = if nitro_param.bootstrap {
            bootstrap(cur_dir.clone().join("secrets"), conn.clone()).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(NitroResponse::default())
        };

        if nitro_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            nitro_proof = prove(conn.clone(), input.clone(), nitro_param.instance_id).await
        }

        nitro_proof.map(|r| r.into())
    }

    async fn aggregate(
        _input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        todo!();
    }

    async fn cancel(_proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }
}

async fn setup(cur_dir: &Path) -> ProverResult<(), String> {
    // launch the nitro guest, should be done only once
    Ok(())
}

pub async fn check_bootstrap(
    secret_dir: PathBuf,
    mut gramine_cmd: StdCommand,
) -> ProverResult<(), ProverError> {
    tokio::task::spawn_blocking(move || {
        // Check if the private key exists
        let path = secret_dir.join(PRIV_KEY_FILENAME);
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

pub async fn bootstrap(
    secret_dir: PathBuf,
    host_connection: SharedHostConnection,
) -> ProverResult<NitroResponse> {
    // todo: async mode
    let conn_cloned = host_connection.clone();
    tokio::task::spawn_blocking(move || {
        let response = conn_cloned
            .send_command(NitroCommand::ExecuteTask {
                task_id: "boostrap".to_owned(),
                task_type: nitro_common::TaskType::Bootstrap,
                inputs: vec![],
            })
            .map_err(|e| {
                ProverError::GuestError(format!("Could not send bootstrap command: {e}"))
            })?;

        let prover_result = match response {
            nitro_common::Response::TaskStatus {
                task_id,
                status,
                result,
                progress,
                error,
            } => result
                .map(|result| NitroResponse {
                    proof: "".to_owned(),
                    quote: result,
                    input: Default::default(),
                })
                .ok_or(ProverError::GuestError(format!(
                    "Bootstrap failed: {error:?}"
                ))),
            _ => Err(ProverError::GuestError(
                "Unexpected response from SGX bootstrap".to_string(),
            )),
        };
        prover_result
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn prove(
    host_connection: SharedHostConnection,
    input: GuestInput,
    instance_id: u64,
) -> ProverResult<NitroResponse, ProverError> {
    let conn_cloned = host_connection.clone();
    tokio::task::spawn_blocking(move || {
        let ser_input = bincode::serialize(&input).map_err(|e| e.to_string())?;
        let response = conn_cloned.send_command(NitroCommand::ExecuteTask {
            task_id: "prove".to_owned(),
            task_type: nitro_common::TaskType::OneShot,
            inputs: ser_input,
        });

        match response {
            Ok(nitro_common::Response::TaskStatus {
                task_id,
                status,
                result,
                progress,
                error,
            }) => result
                .map(|result| NitroResponse {
                    proof: "".to_owned(),
                    quote: result,
                    input: Default::default(),
                })
                .ok_or(ProverError::GuestError(format!("Prove failed: {error:?}"))),
            Ok(_) => Err(ProverError::GuestError(
                "Unexpected response from SGX prove".to_string(),
            )),
            Err(e) => Err(ProverError::GuestError(format!(
                "Could not send prove command: {e}"
            ))),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

async fn aggregate(
    mut gramine_cmd: StdCommand,
    input: AggregationGuestInput,
    instance_id: u64,
) -> ProverResult<NitroResponse, ProverError> {
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
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        let input_success = bincode::serialize_into(stdin, &raw_input);
        let output_success = child.wait_with_output();

        match (input_success, output_success) {
            (Ok(_), Ok(output)) => {
                handle_output(&output, "SGX prove")?;
                Ok(parse_sgx_result(output.stdout)?)
            }
            (Err(i), output_success) => Err(ProverError::GuestError(format!(
                "Can not serialize input for SGX {i}, output is {output_success:?}"
            ))),
            (Ok(_), Err(output_err)) => Err(ProverError::GuestError(
                handle_gramine_error("Could not run SGX guest prover", output_err).to_string(),
            )),
        }
    })
    .await
    .map_err(|e| ProverError::GuestError(e.to_string()))?
}

fn parse_sgx_result(output: Vec<u8>) -> ProverResult<NitroResponse, String> {
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

    Ok(NitroResponse {
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
    println!("{name} stderr: {}", str::from_utf8(&output.stderr).unwrap());
    println!("{name} stdout: {}", str::from_utf8(&output.stdout).unwrap());
    if !output.status.success() {
        return Err(format!(
            "{name} encountered an error ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(())
}
