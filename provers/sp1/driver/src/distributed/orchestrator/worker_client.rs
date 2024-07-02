use async_channel::{Receiver, Sender};
use raiko_lib::prover::WorkerError;
use sp1_core::{runtime::ExecutionState, stark::ShardProof, utils::BabyBearPoseidon2};

use crate::{
    distributed::partial_proof_request::PartialProofRequest, WorkerProtocol, WorkerSocket,
};

pub struct WorkerClient {
    /// The id of the worker
    id: usize,
    /// The url of the worker
    url: String,
    /// A queue to receive the checkpoint to compute the partial proof
    queue: Receiver<(usize, ExecutionState)>,
    /// A channel to send back the id of the checkpoint along with the json strings encoding the computed partial proofs
    answer: Sender<(
        usize,
        Result<Vec<ShardProof<BabyBearPoseidon2>>, WorkerError>,
    )>,
    /// The partial proof request containing the checkpoint data and the challenger
    partial_proof_request: PartialProofRequest,
}

impl WorkerClient {
    pub fn new(
        id: usize,
        url: String,
        queue: Receiver<(usize, ExecutionState)>,
        answer: Sender<(
            usize,
            Result<Vec<ShardProof<BabyBearPoseidon2>>, WorkerError>,
        )>,
        partial_proof_request: PartialProofRequest,
    ) -> Self {
        WorkerClient {
            id,
            url,
            queue,
            answer,
            partial_proof_request,
        }
    }

    pub async fn run(&self) {
        while let Ok((i, checkpoint)) = self.queue.recv().await {
            let partial_proof_result = self.send_work_tcp(i, checkpoint).await;

            if let Err(e) = partial_proof_result {
                log::error!(
                    "Error while sending checkpoint to worker {}: {}. {}",
                    self.id,
                    self.url,
                    e,
                );

                self.answer.send((i, Err(e))).await.unwrap();

                return;
            }

            self.answer.send((i, partial_proof_result)).await.unwrap();
        }

        log::debug!("Worker {} finished", self.id);
    }

    async fn send_work_tcp(
        &self,
        i: usize,
        checkpoint: ExecutionState,
    ) -> Result<Vec<ShardProof<BabyBearPoseidon2>>, WorkerError> {
        let mut socket = WorkerSocket::connect(&self.url).await?;

        log::info!(
            "Sending checkpoint {} to worker {} at {}",
            i,
            self.id,
            self.url
        );

        let mut request = self.partial_proof_request.clone();

        request.checkpoint_id = i;
        request.checkpoint_data = checkpoint;

        socket
            .send(WorkerProtocol::PartialProofRequest(request))
            .await?;

        let response = socket.receive().await?;

        if let WorkerProtocol::PartialProofResponse(partial_proofs) = response {
            Ok(partial_proofs)
        } else {
            Err(WorkerError::InvalidResponse)
        }
    }
}
