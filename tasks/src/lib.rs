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
use utoipa::ToSchema;

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
#[derive(PartialEq, Debug, Copy, Clone, IntoPrimitive, FromPrimitive, Serialize, ToSchema)]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct TaskDescriptor {
    pub chain_id: ChainId,
    pub blockhash: B256,
    pub proof_system: ProofType,
    pub prover: String,
}

impl From<(ChainId, B256, ProofType, String)> for TaskDescriptor {
    fn from(
        (chain_id, blockhash, proof_system, prover): (ChainId, B256, ProofType, String),
    ) -> Self {
        TaskDescriptor {
            chain_id,
            blockhash,
            proof_system,
            prover,
        }
    }
}

/// Task status triplet (status, proof, timestamp).
pub type TaskProvingStatus = (TaskStatus, Option<String>, DateTime<Utc>);

pub type TaskProvingStatusRecords = Vec<TaskProvingStatus>;

#[derive(Debug, Clone)]
pub struct TaskManagerOpts {
    pub sqlite_file: PathBuf,
    pub max_db_size: usize,
}

pub type TaskReport = (TaskDescriptor, TaskStatus);

#[async_trait::async_trait]
pub trait TaskManager {
    /// Create a new task manager.
    fn new(opts: &TaskManagerOpts) -> Self;

    /// Enqueue a new task to the tasks database.
    async fn enqueue_task(
        &mut self,
        request: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords>;

    /// Update a specific tasks progress.
    async fn update_task_progress(
        &mut self,
        key: TaskDescriptor,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()>;

    /// Returns the latest triplet (status, proof - if any, last update time).
    async fn get_task_proving_status(
        &mut self,
        key: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords>;

    /// Returns the proof for the given task.
    async fn get_task_proof(&mut self, key: &TaskDescriptor) -> TaskManagerResult<Vec<u8>>;

    /// Returns the total and detailed database size.
    async fn get_db_size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)>;

    /// Prune old tasks.
    async fn prune_db(&mut self) -> TaskManagerResult<()>;

    /// List all tasks in the db.
    async fn list_all_tasks(&mut self) -> TaskManagerResult<Vec<TaskReport>>;
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

#[async_trait::async_trait]
impl TaskManager for TaskManagerWrapper {
    fn new(opts: &TaskManagerOpts) -> Self {
        let manager = if cfg!(feature = "sqlite") {
            TaskManagerInstance::Sqlite(SqliteTaskManager::new(opts))
        } else {
            TaskManagerInstance::InMemory(InMemoryTaskManager::new(opts))
        };

        Self { manager }
    }

    async fn enqueue_task(
        &mut self,
        request: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.enqueue_task(request).await,
            TaskManagerInstance::Sqlite(ref mut manager) => manager.enqueue_task(request).await,
        }
    }

    async fn update_task_progress(
        &mut self,
        key: TaskDescriptor,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.update_task_progress(key, status, proof).await
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.update_task_progress(key, status, proof).await
            }
        }
    }

    async fn get_task_proving_status(
        &mut self,
        key: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.get_task_proving_status(key).await
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.get_task_proving_status(key).await
            }
        }
    }

    async fn get_task_proof(&mut self, key: &TaskDescriptor) -> TaskManagerResult<Vec<u8>> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.get_task_proof(key).await,
            TaskManagerInstance::Sqlite(ref mut manager) => manager.get_task_proof(key).await,
        }
    }

    async fn get_db_size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.get_db_size().await,
            TaskManagerInstance::Sqlite(ref mut manager) => manager.get_db_size().await,
        }
    }

    async fn prune_db(&mut self) -> TaskManagerResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.prune_db().await,
            TaskManagerInstance::Sqlite(ref mut manager) => manager.prune_db().await,
        }
    }

    async fn list_all_tasks(&mut self) -> TaskManagerResult<Vec<TaskReport>> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.list_all_tasks().await,
            TaskManagerInstance::Sqlite(ref mut manager) => manager.list_all_tasks().await,
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

    #[tokio::test]
    async fn test_new_taskmanager() {
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
                .enqueue_task(&TaskDescriptor {
                    chain_id: 1,
                    blockhash: B256::default(),
                    proof_system: ProofType::Native,
                    prover: "test".to_string(),
                })
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
