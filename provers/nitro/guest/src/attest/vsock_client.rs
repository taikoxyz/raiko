use anyhow::Error;
use log::{error, info};
use nitro_common::{Command, Response, TaskStatus, TaskType};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, io::Read};
use vsock::{VsockAddr, VsockStream};

#[cfg(feature = "aws-nitro")]
const VMADDR_CID_HOST: u32 = 3;
#[cfg(not(feature = "aws-nitro"))]
const VMADDR_CID_HOST: u32 = 2; //for local debug, not working now, todo

pub struct Client {
    cmd_stream: VsockStream,
    tasks: Arc<Mutex<HashMap<String, Response>>>,
}

impl Client {
    pub fn new(cmd_port: u32) -> Result<Self, Error> {
        Ok(Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            cmd_stream: VsockStream::connect(&VsockAddr::new(VMADDR_CID_HOST, cmd_port))?, // 3 是 AWS Nitro Enclave 中的 parent instance
        })
    }

    pub fn run(&self) -> Result<(), Error> {
        let tasks = Arc::clone(&self.tasks);
        let mut cmd_stream = self.cmd_stream.try_clone().map_err(|e| {
            eprintln!("Error cloning command stream: {}", e);
            Error::msg(e.to_string())
        })?;

        std::thread::spawn(move || {
            // todo: refine command message read
            let mut buf = vec![0; 1024 * 1024 * 100];
            loop {
                match cmd_stream.read(&mut buf) {
                    Ok(0) => break, // conn closed
                    Ok(n) => {
                        if let Ok(command) = serde_json::from_slice::<Command>(&buf[..n]) {
                            let response = handle_command(&command, &tasks);
                            if let Ok(resp_bytes) = serde_json::to_vec(&response) {
                                let _ = cmd_stream.write_all(&resp_bytes);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading command: {}", e);
                        break;
                    }
                }
            }
        });

        // todo: log stream to host
        Ok(())
    }
}

fn handle_command(command: &Command, tasks: &Arc<Mutex<HashMap<String, Response>>>) -> Response {
    match command {
        Command::ExecuteTask {
            task_id,
            task_type,
            inputs,
        } => {
            // add task to tasks
            let mut tasks_guard = tasks.lock().unwrap();
            tasks_guard.insert(
                task_id.clone(),
                Response::TaskStatus {
                    task_id: task_id.clone(),
                    status: TaskStatus::Running,
                    progress: None,
                    error: None,
                    result: None,
                },
            );

            // simulate task
            let tasks = Arc::clone(&tasks);
            let task_id_clone = task_id.clone();
            let task_type_clone = task_type.clone();
            let inputs_clone = inputs.clone();
            let handler = std::thread::spawn(move || {
                let mut tasks = tasks.lock().unwrap();

                let response = match task_type_clone {
                    TaskType::Bootstrap => {
                        let opts: crate::app_args::GlobalOpts = crate::app_args::GlobalOpts {
                            secrets_dir: "/tmp".into(),
                            config_dir: "/tmp".into(),
                        };
                        crate::attest::cmd_handler::bootstrap(opts)
                            .map_err(|e| Error::msg(e.to_string()))
                    }
                    TaskType::OneShot => {
                        // todo run one shot task
                        let opts: crate::app_args::GlobalOpts = crate::app_args::GlobalOpts {
                            secrets_dir: "/tmp".into(),
                            config_dir: "/tmp".into(),
                        };
                        let args = crate::app_args::OneShotArgs { sgx_instance_id: 0 };
                        let guest_input: raiko_lib::input::GuestInput =
                            bincode::deserialize_from(&inputs_clone[..])
                                .expect("unable to deserialize input");

                        crate::attest::cmd_handler::one_shot(opts, args, guest_input)
                            .map_err(|e| Error::msg(e.to_string()))
                    }
                    TaskType::Aggregation => {
                        // todo run aggregation task
                        Ok("".to_owned())
                    }
                    _ => Err(Error::msg("Unsupported task type".to_string())),
                };

                tasks.insert(
                    task_id_clone.to_owned(),
                    Response::TaskStatus {
                        task_id: task_id_clone,
                        status: TaskStatus::Completed,
                        progress: None,
                        error: None,
                        result: None,
                    },
                );
                response
            });

            // todo: sync mode for POC only, need refine
            match handler.join() {
                Ok(response) => {
                    info!("Got value: {:?}", response);
                }
                Err(e) => {
                    // 线程 panic 了
                    error!("Thread panicked: {:?}", e);
                }
            }

            Response::TaskStatus {
                task_id: task_id.clone(),
                status: TaskStatus::Completed,
                progress: None,
                error: None,
                result: None,
            }
        }
        Command::QueryTask { task_id } => {
            let tasks = tasks.lock().unwrap();
            tasks
                .get(task_id)
                .cloned()
                .unwrap_or_else(|| Response::Error {
                    code: 404,
                    message: "unknown".to_owned(),
                })
        }
        _ => Response::Error {
            code: 400,
            message: "Unsupported command".to_string(),
        },
    }
}
