use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    // create a new key
    Bootstrap,
    // generate single block proof
    OneShot,
    // aggregate multiple block proofs
    Aggregation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    // health check
    Ping,

    // execute task
    ExecuteTask {
        task_id: String,
        task_type: TaskType,
        inputs: Vec<u8>,
    },

    // query task status
    QueryTask {
        task_id: String,
    },

    // cancel task
    CancelTask {
        task_id: String,
    },

    // get system status
    GetStatus {
        items: Vec<StatusItem>,
    },

    // update configuration
    UpdateConfig {
        config: HashMap<String, String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StatusItem {
    Memory,
    Cpu,
    Tasks,
    Network,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    // health check response
    Pong {
        timestamp: u64,
    },

    // task execution response
    TaskStarted {
        task_id: String,
        estimated_duration: Option<u64>,
    },

    // task status response
    TaskStatus {
        task_id: String,
        status: TaskStatus,
        progress: Option<u32>,
        error: Option<String>,
        result: Option<String>,
    },

    // task cancellation response
    TaskCancelled {
        task_id: String,
        success: bool,
        error: Option<String>,
    },

    // configuration update response
    ConfigUpdated {
        success: bool,
        error: Option<String>,
    },

    // error response
    Error {
        code: u32,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}

impl Command {
    // create a exectution command
    pub fn execute_task(task_id: String, task_type: TaskType) -> Self {
        Command::ExecuteTask {
            task_id,
            task_type,
            inputs: vec![],
        }
    }

    // create a execution command with parameters
    pub fn execute_task_with_params(
        task_id: String,
        task_type: TaskType,
        parameters: Vec<u8>,
    ) -> Self {
        Command::ExecuteTask {
            task_id,
            task_type,
            inputs: parameters,
        }
    }

    // query task status
    pub fn query_task(task_id: String) -> Self {
        Command::QueryTask { task_id }
    }

    // query all status
    pub fn get_all_status() -> Self {
        Command::GetStatus {
            items: vec![StatusItem::All],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogMessage {
    pub timestamp: u64,
    pub level: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_commands() {
        // create a execution command
        let cmd = Command::execute_task_with_params(
            "task-123".to_string(),
            TaskType::OneShot,
            Vec::<u8>::new(),
        );

        // query task status
        let status_cmd = Command::GetStatus {
            items: vec![StatusItem::Memory, StatusItem::Cpu],
        };

        // serialize and deserialize
        let serialized = serde_json::to_string(&cmd).unwrap();
        let deserialized: Command = serde_json::from_str(&serialized).unwrap();
    }
}
