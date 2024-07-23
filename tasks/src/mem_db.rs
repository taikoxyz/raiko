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

use chrono::Utc;
use raiko_lib::prover::{IdStore, IdWrite, ProofKey, ProverError, ProverResult};
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::{
    ensure, TaskDescriptor, TaskManager, TaskManagerError, TaskManagerOpts, TaskManagerResult,
    TaskProvingStatusRecords, TaskReport, TaskStatus,
};

#[derive(Debug)]
pub struct InMemoryTaskManager {
    db: Arc<Mutex<InMemoryTaskDb>>,
}

#[derive(Debug)]
pub struct InMemoryTaskDb {
    enqueue_task: HashMap<TaskDescriptor, TaskProvingStatusRecords>,
    store: HashMap<ProofKey, String>,
}

impl InMemoryTaskDb {
    fn new() -> InMemoryTaskDb {
        InMemoryTaskDb {
            enqueue_task: HashMap::new(),
            store: HashMap::new(),
        }
    }

    fn enqueue_task(&mut self, key: &TaskDescriptor) {
        let task_status = (TaskStatus::Registered, None, Utc::now());

        match self.enqueue_task.get(key) {
            Some(task_proving_records) => {
                debug!(
                    "Task already exists: {:?}",
                    task_proving_records.last().unwrap().0
                );
            } // do nothing
            None => {
                info!("Enqueue new task: {key:?}");
                self.enqueue_task.insert(key.clone(), vec![task_status]);
            }
        }
    }

    fn update_task_progress(
        &mut self,
        key: TaskDescriptor,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        ensure(self.enqueue_task.contains_key(&key), "no task found")?;

        self.enqueue_task.entry(key).and_modify(|entry| {
            if let Some(latest) = entry.last() {
                if latest.0 != status {
                    entry.push((status, proof.map(hex::encode), Utc::now()));
                }
            }
        });

        Ok(())
    }

    fn get_task_proving_status(
        &mut self,
        key: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        Ok(self.enqueue_task.get(key).cloned().unwrap_or_default())
    }

    fn get_task_proof(&mut self, key: &TaskDescriptor) -> TaskManagerResult<Vec<u8>> {
        ensure(self.enqueue_task.contains_key(key), "no task found")?;

        let proving_status_records = self
            .enqueue_task
            .get(key)
            .ok_or_else(|| TaskManagerError::SqlError("no task in db".to_owned()))?;

        let (_, proof, ..) = proving_status_records
            .iter()
            .filter(|(status, ..)| (status == &TaskStatus::Success))
            .last()
            .ok_or_else(|| TaskManagerError::SqlError("no successful task in db".to_owned()))?;

        let Some(proof) = proof else {
            return Ok(vec![]);
        };

        hex::decode(proof)
            .map_err(|_| TaskManagerError::SqlError("couldn't decode from hex".to_owned()))
    }

    fn size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        Ok((self.enqueue_task.len(), vec![]))
    }

    fn prune(&mut self) -> TaskManagerResult<()> {
        self.enqueue_task.clear();
        Ok(())
    }

    fn list_all_tasks(&mut self) -> TaskManagerResult<Vec<TaskReport>> {
        Ok(self
            .enqueue_task
            .iter()
            .flat_map(|(descriptor, statuses)| {
                statuses.last().map(|status| (descriptor.clone(), status.0))
            })
            .collect())
    }

    fn store_id(&mut self, key: ProofKey, id: String) -> TaskManagerResult<()> {
        self.store.insert(key, id);
        Ok(())
    }

    fn remove_id(&mut self, key: ProofKey) -> TaskManagerResult<()> {
        self.store.remove(&key);
        Ok(())
    }

    fn read_id(&mut self, key: ProofKey) -> TaskManagerResult<String> {
        self.store
            .get(&key)
            .cloned()
            .ok_or_else(|| TaskManagerError::SqlError("no id found".to_owned()))
    }
}

#[async_trait::async_trait]
impl IdWrite for InMemoryTaskManager {
    async fn store_id(&mut self, key: ProofKey, id: String) -> ProverResult<()> {
        let mut db = self.db.lock().await;
        db.store_id(key, id)
            .map_err(|e| ProverError::StoreError(e.to_string()))
    }

    async fn remove_id(&mut self, key: ProofKey) -> ProverResult<()> {
        let mut db = self.db.lock().await;
        db.remove_id(key)
            .map_err(|e| ProverError::StoreError(e.to_string()))
    }
}

#[async_trait::async_trait]
impl IdStore for InMemoryTaskManager {
    async fn read_id(&self, key: ProofKey) -> ProverResult<String> {
        let mut db = self.db.lock().await;
        db.read_id(key)
            .map_err(|e| ProverError::StoreError(e.to_string()))
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
        params: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let mut db = self.db.lock().await;
        let status = db.get_task_proving_status(params)?;
        if !status.is_empty() {
            return Ok(status);
        }

        db.enqueue_task(params);
        db.get_task_proving_status(params)
    }

    async fn update_task_progress(
        &mut self,
        key: TaskDescriptor,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        let mut db = self.db.lock().await;
        db.update_task_progress(key, status, proof)
    }

    /// Returns the latest triplet (submitter or fulfiller, status, last update time)
    async fn get_task_proving_status(
        &mut self,
        key: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let mut db = self.db.lock().await;
        db.get_task_proving_status(key)
    }

    async fn get_task_proof(&mut self, key: &TaskDescriptor) -> TaskManagerResult<Vec<u8>> {
        let mut db = self.db.lock().await;
        db.get_task_proof(key)
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
    use alloy_primitives::B256;

    use super::*;
    use crate::ProofType;

    #[test]
    fn test_db_open() {
        assert!(InMemoryTaskDb::new().size().is_ok());
    }

    #[test]
    fn test_db_enqueue() {
        let mut db = InMemoryTaskDb::new();
        let params = TaskDescriptor {
            chain_id: 1,
            blockhash: B256::default(),
            proof_system: ProofType::Native,
            prover: "0x1234".to_owned(),
        };
        db.enqueue_task(&params);
        let status = db.get_task_proving_status(&params);
        assert!(status.is_ok());
    }
}
