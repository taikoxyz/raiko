// Raiko
// Copyright (c) 2024 Taiko Labs
// Licensed and distributed under either of
//   * MIT license (license terms in the root directory or at http://opensource.org/licenses/MIT).
//   * Apache v2 license (license terms in the root directory or at http://www.apache.org/licenses/LICENSE-2.0).
// at your option. This file may not be copied, modified, or distributed except according to those terms.

// Imports
// ----------------------------------------------------------------
use std::{
    collections::HashMap,
    sync::{Arc, Once},
};

use crate::{
    ensure, EnqueueTaskParams, TaskDescriptor, TaskManager, TaskManagerError, TaskManagerOpts,
    TaskManagerResult, TaskProvingStatus, TaskProvingStatusRecords, TaskReport, TaskStatus,
};

use chrono::Utc;
use raiko_core::interfaces::ProofType;
use raiko_lib::primitives::{ChainId, B256};
use tokio::sync::Mutex;
use tracing::{debug, info};

#[derive(Debug)]
pub struct InMemoryTaskManager {
    db: Arc<Mutex<InMemoryTaskDb>>,
}

#[derive(Debug)]
pub struct InMemoryTaskDb {
    enqueue_task: HashMap<TaskDescriptor, TaskProvingStatusRecords>,
}

impl InMemoryTaskDb {
    fn new() -> InMemoryTaskDb {
        InMemoryTaskDb {
            enqueue_task: HashMap::new(),
        }
    }

    fn enqueue_task(&mut self, params: &EnqueueTaskParams) {
        let key = TaskDescriptor::from(params);
        let task_status = TaskProvingStatus(
            TaskStatus::Registered,
            Some(params.prover.clone()),
            Utc::now(),
        );

        match self.enqueue_task.get(&key) {
            Some(task_proving_records) => {
                debug!(
                    "Task already exists: {:?}",
                    task_proving_records.last().unwrap().0
                );
            } // do nothing
            None => {
                info!("Enqueue new task: {:?}", params);
                self.enqueue_task.insert(key, vec![task_status]);
            }
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
        let key = TaskDescriptor::from((chain_id, blockhash, proof_system, prover.clone()));
        ensure(self.enqueue_task.contains_key(&key), "no task found")?;

        self.enqueue_task.entry(key).and_modify(|entry| {
            if let Some(latest) = entry.last() {
                if latest.0 != status {
                    entry.push(TaskProvingStatus(
                        status,
                        proof.map(hex::encode),
                        Utc::now(),
                    ));
                }
            }
        });
        Ok(())
    }

    fn get_task_proving_status(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let key = TaskDescriptor::from((chain_id, blockhash, proof_system, prover.clone()));

        match self.enqueue_task.get(&key) {
            Some(proving_status_records) => Ok(proving_status_records.clone()),
            None => Ok(vec![]),
        }
    }

    fn get_task_proof(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<Vec<u8>> {
        let key = TaskDescriptor::from((chain_id, blockhash, proof_system, prover.clone()));
        ensure(self.enqueue_task.contains_key(&key), "no task found")?;

        let Some(proving_status_records) = self.enqueue_task.get(&key) else {
            return Err(TaskManagerError::SqlError("no task in db".to_owned()));
        };

        proving_status_records
            .last()
            .map(|status| hex::decode(status.1.clone().unwrap()).unwrap())
            .ok_or_else(|| TaskManagerError::SqlError("working in progress".to_owned()))
    }

    fn size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        Ok((self.enqueue_task.len(), vec![]))
    }

    fn prune(&mut self) -> TaskManagerResult<()> {
        Ok(())
    }

    fn list_all_tasks(&mut self) -> TaskManagerResult<Vec<TaskReport>> {
        Ok(self
            .enqueue_task
            .iter()
            .flat_map(|(descriptor, statuses)| {
                // list only the latest status
                statuses
                    .last()
                    .map(|status| TaskReport(descriptor.clone(), status.0))
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl TaskManager for InMemoryTaskManager {
    fn new(_opts: &TaskManagerOpts) -> Self {
        static INIT: Once = Once::new();
        static mut SHARED_TASK_MANAGER: Option<Arc<Mutex<InMemoryTaskDb>>> = None;

        INIT.call_once(|| {
            let task_manager: Arc<Mutex<InMemoryTaskDb>> =
                Arc::new(Mutex::new(InMemoryTaskDb::new()));
            unsafe {
                SHARED_TASK_MANAGER = Some(Arc::clone(&task_manager));
            }
        });

        InMemoryTaskManager {
            db: unsafe { SHARED_TASK_MANAGER.clone().unwrap() },
        }
    }

    async fn enqueue_task(
        &mut self,
        params: &EnqueueTaskParams,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let mut db = self.db.lock().await;
        let status = db.get_task_proving_status(
            params.chain_id,
            params.blockhash,
            params.proof_type,
            Some(params.prover.to_string()),
        )?;
        if status.is_empty() {
            db.enqueue_task(params);
            db.get_task_proving_status(
                params.chain_id,
                params.blockhash,
                params.proof_type,
                Some(params.prover.clone()),
            )
        } else {
            Ok(status)
        }
    }

    async fn update_task_progress(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        let mut db = self.db.lock().await;
        db.update_task_progress(chain_id, blockhash, proof_system, prover, status, proof)
    }

    /// Returns the latest triplet (submitter or fulfiller, status, last update time)
    async fn get_task_proving_status(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let mut db = self.db.lock().await;
        db.get_task_proving_status(chain_id, blockhash, proof_system, prover)
    }

    async fn get_task_proof(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<Vec<u8>> {
        let mut db = self.db.lock().await;
        db.get_task_proof(chain_id, blockhash, proof_system, prover)
    }

    /// Returns the total and detailed database size
    async fn get_db_size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        let mut db = self.db.lock().await;
        db.size()
    }

    async fn prune_db(&mut self) -> TaskManagerResult<()> {
        let mut db = self.db.lock().await;
        db.prune()
    }

    async fn list_all_tasks(&mut self) -> TaskManagerResult<Vec<TaskReport>> {
        let mut db = self.db.lock().await;
        db.list_all_tasks()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProofType;

    #[test]
    fn test_db_open() {
        assert!(InMemoryTaskDb::new().size().is_ok());
    }

    #[test]
    fn test_db_enqueue() {
        let mut db = InMemoryTaskDb::new();
        let params = EnqueueTaskParams {
            chain_id: 1,
            blockhash: B256::default(),
            proof_type: ProofType::Native,
            prover: "0x1234".to_owned(),
            ..Default::default()
        };
        db.enqueue_task(&params);
        let status = db.get_task_proving_status(
            params.chain_id,
            params.blockhash,
            params.proof_type,
            Some(params.prover.clone()),
        );
        assert!(status.is_ok());
    }
}
