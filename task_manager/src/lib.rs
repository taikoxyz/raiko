use std::{
    io::{Error as IOError, ErrorKind as IOErrorKind},
    path::PathBuf,
};

use chrono::{DateTime, Utc};
use num_enum::{FromPrimitive, IntoPrimitive};
use raiko_core::interfaces::ProofType;
use raiko_lib::primitives::{ChainId, B256};
use rusqlite::Error as SqlError;
use serde::Serialize;

use crate::{adv_sqlite::SqliteTaskManager, mem_db::InMemoryTaskManager};

mod adv_sqlite;
mod mem_db;

// Types
// ----------------------------------------------------------------
#[derive(PartialEq, Debug, thiserror::Error)]
pub enum TaskManagerError {
    #[error("IO Error {0}")]
    IOError(IOErrorKind),
    #[error("SQL Error {0}")]
    SqlError(String),
    #[error("Anyhow error: {0}")]
    Anyhow(String),
}

pub type TaskManagerResult<T> = Result<T, TaskManagerError>;

impl From<IOError> for TaskManagerError {
    fn from(error: IOError) -> TaskManagerError {
        TaskManagerError::IOError(error.kind())
    }
}

impl From<SqlError> for TaskManagerError {
    fn from(error: SqlError) -> TaskManagerError {
        TaskManagerError::SqlError(error.to_string())
    }
}

impl From<serde_json::Error> for TaskManagerError {
    fn from(error: serde_json::Error) -> TaskManagerError {
        TaskManagerError::SqlError(error.to_string())
    }
}

impl From<anyhow::Error> for TaskManagerError {
    fn from(value: anyhow::Error) -> Self {
        TaskManagerError::Anyhow(value.to_string())
    }
}

#[allow(non_camel_case_types)]
#[rustfmt::skip]
#[derive(PartialEq, Debug, Copy, Clone, IntoPrimitive, FromPrimitive, Serialize)]
#[repr(i32)]
pub enum TaskStatus {
    Success                   =     0,
    Registered                =  1000,
    WorkInProgress            =  2000,
    ProofFailure_Generic      = -1000,
    ProofFailure_OutOfMemory  = -1100,
    NetworkFailure            = -2000,
    Cancelled                 = -3000,
    Cancelled_NeverStarted    = -3100,
    Cancelled_Aborted         = -3200,
    CancellationInProgress    = -3210,
    InvalidOrUnsupportedBlock = -4000,
    UnspecifiedFailureReason  = -9999,
    #[num_enum(default)]
    SqlDbCorruption           = -99999,
}

#[derive(Debug, Clone, Default)]
pub struct EnqueueTaskParams {
    pub chain_id: ChainId,
    pub blockhash: B256,
    pub proof_type: ProofType,
    pub prover: String,
    pub block_number: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskDescriptor {
    pub chain_id: ChainId,
    pub blockhash: B256,
    pub proof_system: ProofType,
    pub prover: String,
}

impl TaskDescriptor {
    pub fn to_vec(self) -> Vec<u8> {
        self.into()
    }
}

impl From<TaskDescriptor> for Vec<u8> {
    fn from(val: TaskDescriptor) -> Self {
        let mut v = Vec::new();
        v.extend_from_slice(&val.chain_id.to_be_bytes());
        v.extend_from_slice(val.blockhash.as_ref());
        v.extend_from_slice(&(val.proof_system as u8).to_be_bytes());
        v.extend_from_slice(val.prover.as_bytes());
        v
    }
}

// Taskkey from EnqueueTaskParams
impl From<&EnqueueTaskParams> for TaskDescriptor {
    fn from(params: &EnqueueTaskParams) -> TaskDescriptor {
        TaskDescriptor {
            chain_id: params.chain_id,
            blockhash: params.blockhash,
            proof_system: params.proof_type,
            prover: params.prover.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskProvingStatus(pub TaskStatus, pub Option<String>, pub DateTime<Utc>);

pub type TaskProvingStatusRecords = Vec<TaskProvingStatus>;

#[derive(Debug, Clone)]
pub struct TaskManagerOpts {
    pub sqlite_file: PathBuf,
    pub max_db_size: usize,
}

#[async_trait::async_trait]
pub trait TaskManager {
    /// new a task manager
    fn new(opts: &TaskManagerOpts) -> Self;

    /// enqueue_task
    async fn enqueue_task(
        &mut self,
        request: &EnqueueTaskParams,
    ) -> TaskManagerResult<TaskProvingStatusRecords>;

    /// Update the task progress
    async fn update_task_progress(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()>;

    /// Returns the latest triplet (submitter or fulfiller, status, last update time)
    async fn get_task_proving_status(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<TaskProvingStatusRecords>;

    /// Returns the latest triplet (submitter or fulfiller, status, last update time)
    async fn get_task_proving_status_by_id(
        &mut self,
        task_id: u64,
    ) -> TaskManagerResult<TaskProvingStatusRecords>;

    /// Returns the proof for the given task
    async fn get_task_proof(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<Vec<u8>>;

    async fn get_task_proof_by_id(&mut self, task_id: u64) -> TaskManagerResult<Vec<u8>>;

    /// Returns the total and detailed database size
    async fn get_db_size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)>;

    /// Prune old tasks
    async fn prune_db(&mut self) -> TaskManagerResult<()>;
}

pub fn ensure(expression: bool, message: &str) -> TaskManagerResult<()> {
    if !expression {
        return Err(TaskManagerError::Anyhow(message.to_string()));
    }
    Ok(())
}

enum TaskManagerInstance {
    InMemory(InMemoryTaskManager),
    Sqlite(SqliteTaskManager),
}

pub struct TaskManagerWrapper {
    manager: TaskManagerInstance,
}

impl TaskManager for TaskManagerWrapper {
    fn new(opts: &TaskManagerOpts) -> Self {
        let manager = if cfg!(feature = "sqlite") {
            TaskManagerInstance::Sqlite(SqliteTaskManager::new(opts))
        } else {
            TaskManagerInstance::InMemory(InMemoryTaskManager::new(opts))
        };

        Self { manager }
    }

    fn enqueue_task(
        &mut self,
        request: &EnqueueTaskParams,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.enqueue_task(request),
            TaskManagerInstance::Sqlite(ref mut manager) => manager.enqueue_task(request),
        }
    }

    fn update_task_progress(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.update_task_progress(
                chain_id,
                blockhash,
                proof_system,
                prover,
                status,
                proof,
            ),
            TaskManagerInstance::Sqlite(ref mut manager) => manager.update_task_progress(
                chain_id,
                blockhash,
                proof_system,
                prover,
                status,
                proof,
            ),
        }
    }

    fn get_task_proving_status(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.get_task_proving_status(chain_id, blockhash, proof_system, prover)
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.get_task_proving_status(chain_id, blockhash, proof_system, prover)
            }
        }
    }

    fn get_task_proving_status_by_id(
        &mut self,
        task_id: u64,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.get_task_proving_status_by_id(task_id)
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.get_task_proving_status_by_id(task_id)
            }
        }
    }

    fn get_task_proof(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<Vec<u8>> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.get_task_proof(chain_id, blockhash, proof_system, prover)
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.get_task_proof(chain_id, blockhash, proof_system, prover)
            }
        }
    }

    fn get_task_proof_by_id(&mut self, task_id: u64) -> TaskManagerResult<Vec<u8>> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.get_task_proof_by_id(task_id),
            TaskManagerInstance::Sqlite(ref mut manager) => manager.get_task_proof_by_id(task_id),
        }
    }

    fn get_db_size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.get_db_size(),
            TaskManagerInstance::Sqlite(ref mut manager) => manager.get_db_size(),
        }
    }

    fn prune_db(&mut self) -> TaskManagerResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.prune_db(),
            TaskManagerInstance::Sqlite(ref mut manager) => manager.prune_db(),
        }
    }
}

pub fn get_task_manager(opts: &TaskManagerOpts) -> TaskManagerWrapper {
    TaskManagerWrapper::new(opts)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_new_taskmanager() {
        let sqlite_file: &Path = Path::new("test.db");
        // remove existed one
        if sqlite_file.exists() {
            std::fs::remove_file(sqlite_file).unwrap();
        }

        let opts = TaskManagerOpts {
            sqlite_file: sqlite_file.to_path_buf(),
            max_db_size: 1024 * 1024,
        };
        let mut task_manager = get_task_manager(&opts);

        assert_eq!(
            task_manager
                .enqueue_task(&EnqueueTaskParams {
                    chain_id: 1,
                    blockhash: B256::default(),
                    proof_type: ProofType::Native,
                    prover: "test".to_string(),
                    block_number: 1
                })
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
