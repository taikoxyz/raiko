use serde::{Deserialize, Serialize};

use crate::WorkerProtocol;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerEnvelope {
    pub magic: u64,
    pub data: WorkerProtocol,
}

impl From<WorkerProtocol> for WorkerEnvelope {
    fn from(data: WorkerProtocol) -> Self {
        WorkerEnvelope {
            magic: 0xdeadbeef,
            data,
        }
    }
}
