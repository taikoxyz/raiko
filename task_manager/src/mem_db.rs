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
use raiko_lib::primitives::{keccak::keccak, ChainId, B256};
use tokio::sync::Mutex;
use tracing::{debug, info};

#[derive(Debug)]
pub struct InMemoryTaskManager {
    db: Arc<Mutex<InMemoryTaskDb>>,
}

#[derive(Debug)]
pub struct InMemoryTaskDb {
    enqueue_task: HashMap<B256, TaskProvingStatusRecords>,
    task_id_desc: HashMap<u64, B256>,
    task_id: u64,
}

impl InMemoryTaskDb {
    fn new() -> InMemoryTaskDb {
        InMemoryTaskDb {
            enqueue_task: HashMap::new(),
            task_id_desc: HashMap::new(),
            task_id: 0,
        }
    }

    fn enqueue_task(&mut self, params: &EnqueueTaskParams) {
        let key: B256 = keccak(TaskDescriptor::from(params).to_vec()).into();
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
                self.task_id_desc.insert(self.task_id, key);
                self.task_id += 1;
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
        let key: B256 = keccak(
            TaskDescriptor::from((chain_id, blockhash, proof_system, prover.clone())).to_vec(),
        )
        .into();
        ensure(self.enqueue_task.contains_key(&key), "no task found")?;

        let task_proving_records = self.enqueue_task.get(&key).unwrap();
        let task_status = task_proving_records.last().unwrap().0;
        if status != task_status {
            let new_records = task_proving_records
                .iter()
                .cloned()
                .chain(std::iter::once(TaskProvingStatus(
                    status,
                    proof.map(hex::encode),
                    Utc::now(),
                )))
                .collect();
            self.enqueue_task.insert(key, new_records);
        }
        Ok(())
    }

    fn get_task_proving_status(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let key: B256 = keccak(
            TaskDescriptor::from((chain_id, blockhash, proof_system, prover.clone())).to_vec(),
        )
        .into();

        match self.enqueue_task.get(&key) {
            Some(proving_status_records) => Ok(proving_status_records.clone()),
            None => Ok(vec![]),
        }
    }

    fn get_task_proving_status_by_id(
        &mut self,
        task_id: u64,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        ensure(self.task_id_desc.contains_key(&task_id), "no task found")?;
        let key = self.task_id_desc.get(&task_id).unwrap();
        let task_status = self.enqueue_task.get(key).unwrap();
        Ok(task_status.clone())
    }

    fn get_task_proof(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: ProofType,
        prover: Option<String>,
    ) -> TaskManagerResult<Vec<u8>> {
        let key: B256 = keccak(
            TaskDescriptor::from((chain_id, blockhash, proof_system, prover.clone())).to_vec(),
        )
        .into();
        ensure(self.enqueue_task.contains_key(&key), "no task found")?;

        let proving_status_records = self.enqueue_task.get(&key).unwrap();
        let task_status = proving_status_records.last().unwrap();
        if task_status.0 == TaskStatus::Success {
            let proof = task_status.1.clone().unwrap();
            Ok(hex::decode(proof).unwrap())
        } else {
            Err(TaskManagerError::SqlError("working in process".to_owned()))
        }
    }

    fn get_task_proof_by_id(&mut self, task_id: u64) -> TaskManagerResult<Vec<u8>> {
        ensure(self.task_id_desc.contains_key(&task_id), "no task found")?;
        let key = self.task_id_desc.get(&task_id).unwrap();
        let task_records = self.enqueue_task.get(key).unwrap();
        let task_status = task_records.last().unwrap();
        if task_status.0 == TaskStatus::Success {
            let proof = task_status.1.clone().unwrap();
            Ok(hex::decode(proof).unwrap())
        } else {
            Err(TaskManagerError::SqlError("working in process".to_owned()))
        }
    }

    fn size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        Ok((self.enqueue_task.len() + self.task_id_desc.len(), vec![]))
    }

    fn prune(&mut self) -> TaskManagerResult<()> {
        Ok(())
    }

    fn list_all_tasks(&mut self) -> TaskManagerResult<Vec<TaskReport>> {
        Ok(vec![])
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

    /// Returns the latest triplet (submitter or fulfiller, status, last update time)
    async fn get_task_proving_status_by_id(
        &mut self,
        task_id: u64,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let mut db = self.db.lock().await;
        db.get_task_proving_status_by_id(task_id)
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

    async fn get_task_proof_by_id(&mut self, task_id: u64) -> TaskManagerResult<Vec<u8>> {
        let mut db = self.db.lock().await;
        db.get_task_proof_by_id(task_id)
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
