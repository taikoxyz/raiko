use raiko_core::interfaces::AggregationOnlyRequest;
use raiko_lib::{
    primitives::{ChainId, B256},
    proof_type::ProofType,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
    NetworkFailure(String),
    Cancelled,
    Cancelled_NeverStarted,
    Cancelled_Aborted,
    CancellationInProgress,
    InvalidOrUnsupportedBlock,
    #[serde(rename = "zk_any_not_drawn")]
    ZKAnyNotDrawn,
    IoFailure(String),
    AnyhowError(String),
    GuestProverFailure(String),
    UnspecifiedFailureReason,
    TaskDbCorruption(String),
    SystemPaused,
}

impl From<TaskStatus> for i32 {
    fn from(status: TaskStatus) -> i32 {
        match status {
            TaskStatus::Success => 0,
            TaskStatus::Registered => 1000,
            TaskStatus::WorkInProgress => 2000,
            TaskStatus::ProofFailure_Generic => -1000,
            TaskStatus::ProofFailure_OutOfMemory => -1100,
            TaskStatus::NetworkFailure(_) => -2000,
            TaskStatus::Cancelled => -3000,
            TaskStatus::Cancelled_NeverStarted => -3100,
            TaskStatus::Cancelled_Aborted => -3200,
            TaskStatus::CancellationInProgress => -3210,
            TaskStatus::InvalidOrUnsupportedBlock => -4000,
            TaskStatus::ZKAnyNotDrawn => -4100,
            TaskStatus::IoFailure(_) => -5000,
            TaskStatus::AnyhowError(_) => -6000,
            TaskStatus::GuestProverFailure(_) => -7000,
            TaskStatus::UnspecifiedFailureReason => -8000,
            TaskStatus::TaskDbCorruption(_) => -9000,
            TaskStatus::SystemPaused => -10000,
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
            -2000 => TaskStatus::NetworkFailure("".to_string()),
            -3000 => TaskStatus::Cancelled,
            -3100 => TaskStatus::Cancelled_NeverStarted,
            -3200 => TaskStatus::Cancelled_Aborted,
            -3210 => TaskStatus::CancellationInProgress,
            -4000 => TaskStatus::InvalidOrUnsupportedBlock,
            -4100 => TaskStatus::ZKAnyNotDrawn,
            -5000 => TaskStatus::IoFailure("".to_string()),
            -6000 => TaskStatus::AnyhowError("".to_string()),
            -7000 => TaskStatus::GuestProverFailure("".to_string()),
            -8000 => TaskStatus::UnspecifiedFailureReason,
            -9000 => TaskStatus::TaskDbCorruption("".to_string()),
            -10000 => TaskStatus::SystemPaused,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub struct GuestInputTaskDescriptor {
    pub chain_id: ChainId,
    pub block_id: u64,
    pub blockhash: B256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub struct ProofTaskDescriptor {
    pub chain_id: ChainId,
    pub block_id: u64,
    pub blockhash: B256,
    pub proof_system: ProofType,
    pub prover: String,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
/// A request for proof aggregation of multiple proofs.
pub struct AggregationTaskDescriptor {
    /// The block numbers and l1 inclusion block numbers for the blocks to aggregate proofs for.
    pub aggregation_ids: Vec<u64>,
    /// The proof type.
    pub proof_type: Option<String>,
}

impl From<&AggregationOnlyRequest> for AggregationTaskDescriptor {
    fn from(request: &AggregationOnlyRequest) -> Self {
        Self {
            aggregation_ids: request.aggregation_ids.clone(),
            proof_type: request.proof_type.clone().map(|p| p.to_string()),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
/// A request for proof aggregation of multiple proofs.
pub struct BatchGuestInputTaskDescriptor {
    pub chain_id: ChainId,
    pub batch_id: u64,
    pub l1_height: u64,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
/// A request for proof aggregation of multiple proofs.
pub struct BatchProofTaskDescriptor {
    pub chain_id: ChainId,
    pub batch_id: u64,
    pub l1_height: u64,
    pub proof_system: ProofType,
    pub prover: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// A request for Shasta guest input generation.
pub struct ShastaGuestInputTaskDescriptor {
    pub proposal_id: u64,
    pub l1_network: String,
    pub l2_network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// A request for Shasta proof generation.
pub struct ShastaProofTaskDescriptor {
    pub proposal_id: u64,
    pub l1_network: String,
    pub l2_network: String,
    pub proof_system: ProofType,
    pub prover: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskDescriptor {
    GuestInput(GuestInputTaskDescriptor),
    SingleProof(ProofTaskDescriptor),
    Aggregation(AggregationTaskDescriptor),
    BatchProof(BatchProofTaskDescriptor),
    BatchGuestInput(BatchGuestInputTaskDescriptor),
    ShastaGuestInput(ShastaGuestInputTaskDescriptor),
    ShastaProof(ShastaProofTaskDescriptor),
}

pub type TaskReport = (TaskDescriptor, TaskStatus);

pub type AggregationTaskReport = (AggregationOnlyRequest, TaskStatus);
