use std::{
    io::{Error as IOError, ErrorKind as IOErrorKind},
    path::PathBuf,
};

use chrono::{DateTime, Utc};
use raiko_core::interfaces::{AggregationOnlyRequest, ProofType};
use raiko_lib::{
    primitives::{ChainId, B256},
    prover::{IdStore, IdWrite, ProofKey, ProverResult},
};
use rusqlite::Error as SqlError;
use serde::{Deserialize, Serialize};
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
    #[error("No data for query")]
    NoData,
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
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize, ToSchema, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Success,
    Registered,
    WorkInProgress,
    ProofFailure_Generic,
    ProofFailure_OutOfMemory,
    NetworkFailure,
    Cancelled,
    Cancelled_NeverStarted,
    Cancelled_Aborted,
    CancellationInProgress,
    InvalidOrUnsupportedBlock,
    NonDbFailure(String),
    UnspecifiedFailureReason,
    SqlDbCorruption,
}

impl From<TaskStatus> for i32 {
    fn from(status: TaskStatus) -> i32 {
        match status {
            TaskStatus::Success => 0,
            TaskStatus::Registered => 1000,
            TaskStatus::WorkInProgress => 2000,
            TaskStatus::ProofFailure_Generic => -1000,
            TaskStatus::ProofFailure_OutOfMemory => -1100,
            TaskStatus::NetworkFailure => -2000,
            TaskStatus::Cancelled => -3000,
            TaskStatus::Cancelled_NeverStarted => -3100,
            TaskStatus::Cancelled_Aborted => -3200,
            TaskStatus::CancellationInProgress => -3210,
            TaskStatus::InvalidOrUnsupportedBlock => -4000,
            TaskStatus::NonDbFailure(_) => -5000,
            TaskStatus::UnspecifiedFailureReason => -9999,
            TaskStatus::SqlDbCorruption => -99999,
        }
    }
}

impl From<i32> for TaskStatus {
    fn from(value: i32) -> TaskStatus {
        match value {
            0 => TaskStatus::Success,
            1000 => TaskStatus::Registered,
            2000 => TaskStatus::WorkInProgress,
            -1000 => TaskStatus::ProofFailure_Generic,
            -1100 => TaskStatus::ProofFailure_OutOfMemory,
            -2000 => TaskStatus::NetworkFailure,
            -3000 => TaskStatus::Cancelled,
            -3100 => TaskStatus::Cancelled_NeverStarted,
            -3200 => TaskStatus::Cancelled_Aborted,
            -3210 => TaskStatus::CancellationInProgress,
            -4000 => TaskStatus::InvalidOrUnsupportedBlock,
            -5000 => TaskStatus::NonDbFailure("".to_string()),
            -9999 => TaskStatus::UnspecifiedFailureReason,
            -99999 => TaskStatus::SqlDbCorruption,
            _ => TaskStatus::UnspecifiedFailureReason,
        }
    }
}

impl FromIterator<TaskStatus> for TaskStatus {
    fn from_iter<T: IntoIterator<Item = TaskStatus>>(iter: T) -> Self {
        iter.into_iter()
            .min()
            .unwrap_or(TaskStatus::UnspecifiedFailureReason)
    }
}

impl<'a> FromIterator<&'a TaskStatus> for TaskStatus {
    fn from_iter<T: IntoIterator<Item = &'a TaskStatus>>(iter: T) -> Self {
        iter.into_iter()
            .min()
            .cloned()
            .unwrap_or(TaskStatus::UnspecifiedFailureReason)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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

impl From<TaskDescriptor> for (ChainId, B256) {
    fn from(
        TaskDescriptor {
            chain_id,
            blockhash,
            ..
        }: TaskDescriptor,
    ) -> Self {
        (chain_id, blockhash)
    }
}

/// Task status triplet (status, proof, timestamp).
pub type TaskProvingStatus = (TaskStatus, Option<String>, DateTime<Utc>);

pub type TaskProvingStatusRecords = Vec<TaskProvingStatus>;

pub type TaskReport = (TaskDescriptor, TaskStatus);

#[derive(Debug, Clone, Default)]
pub struct TaskManagerOpts {
    pub sqlite_file: PathBuf,
    pub max_db_size: usize,
}

#[async_trait::async_trait]
pub trait TaskManager: IdStore + IdWrite {
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

    /// List all stored ids.
    async fn list_stored_ids(&mut self) -> TaskManagerResult<Vec<(ProofKey, String)>>;

    /// Enqueue a new aggregation task to the tasks database.
    async fn enqueue_aggregation_task(
        &mut self,
        request: &AggregationOnlyRequest,
    ) -> TaskManagerResult<()>;

    /// Update a specific aggregation tasks progress.
    async fn update_aggregation_task_progress(
        &mut self,
        request: &AggregationOnlyRequest,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()>;

    /// Returns the latest triplet (status, proof - if any, last update time).
    async fn get_aggregation_task_proving_status(
        &mut self,
        request: &AggregationOnlyRequest,
    ) -> TaskManagerResult<TaskProvingStatusRecords>;

    /// Returns the proof for the given aggregation task.
    async fn get_aggregation_task_proof(
        &mut self,
        request: &AggregationOnlyRequest,
    ) -> TaskManagerResult<Vec<u8>>;
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
impl IdWrite for TaskManagerWrapper {
    async fn store_id(&mut self, key: ProofKey, id: String) -> ProverResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.store_id(key, id).await,
            TaskManagerInstance::Sqlite(ref mut manager) => manager.store_id(key, id).await,
        }
    }

    async fn remove_id(&mut self, key: ProofKey) -> ProverResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => manager.remove_id(key).await,
            TaskManagerInstance::Sqlite(ref mut manager) => manager.remove_id(key).await,
        }
    }
}

#[async_trait::async_trait]
impl IdStore for TaskManagerWrapper {
    async fn read_id(&self, key: ProofKey) -> ProverResult<String> {
        match &self.manager {
            TaskManagerInstance::InMemory(manager) => manager.read_id(key).await,
            TaskManagerInstance::Sqlite(manager) => manager.read_id(key).await,
        }
    }
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

    async fn list_stored_ids(&mut self) -> TaskManagerResult<Vec<(ProofKey, String)>> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(manager) => manager.list_stored_ids().await,
            TaskManagerInstance::Sqlite(manager) => manager.list_stored_ids().await,
        }
    }

    async fn enqueue_aggregation_task(
        &mut self,
        request: &AggregationOnlyRequest,
    ) -> TaskManagerResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.enqueue_aggregation_task(request).await
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.enqueue_aggregation_task(request).await
            }
        }
    }

    async fn update_aggregation_task_progress(
        &mut self,
        request: &AggregationOnlyRequest,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager
                    .update_aggregation_task_progress(request, status, proof)
                    .await
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager
                    .update_aggregation_task_progress(request, status, proof)
                    .await
            }
        }
    }

    async fn get_aggregation_task_proving_status(
        &mut self,
        request: &AggregationOnlyRequest,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.get_aggregation_task_proving_status(request).await
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.get_aggregation_task_proving_status(request).await
            }
        }
    }

    async fn get_aggregation_task_proof(
        &mut self,
        request: &AggregationOnlyRequest,
    ) -> TaskManagerResult<Vec<u8>> {
        match &mut self.manager {
            TaskManagerInstance::InMemory(ref mut manager) => {
                manager.get_aggregation_task_proof(request).await
            }
            TaskManagerInstance::Sqlite(ref mut manager) => {
                manager.get_aggregation_task_proof(request).await
            }
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
