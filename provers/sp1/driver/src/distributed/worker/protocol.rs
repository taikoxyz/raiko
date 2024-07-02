use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use sp1_core::{stark::ShardProof, utils::BabyBearPoseidon2};

use crate::PartialProofRequest;

#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerProtocol {
    Ping,
    PartialProofRequest(PartialProofRequest),
    PartialProofResponse(Vec<ShardProof<BabyBearPoseidon2>>),
}

impl Display for WorkerProtocol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerProtocol::Ping => write!(f, "Ping"),
            WorkerProtocol::PartialProofRequest(_) => write!(f, "PartialProofRequest"),
            WorkerProtocol::PartialProofResponse(_) => write!(f, "PartialProofResponse"),
        }
    }
}
