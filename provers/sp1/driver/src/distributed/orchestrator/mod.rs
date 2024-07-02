mod worker_client;

use raiko_lib::prover::WorkerError;
use sp1_core::{runtime::ExecutionState, stark::ShardProof, utils::BabyBearPoseidon2};
use worker_client::WorkerClient;

use super::partial_proof_request::PartialProofRequest;

pub async fn distribute_work(
    ip_list: Vec<String>,
    checkpoints: Vec<ExecutionState>,
    partial_proof_request: PartialProofRequest,
) -> Result<Vec<ShardProof<BabyBearPoseidon2>>, WorkerError> {
    let mut nb_workers = ip_list.len();

    let (queue_tx, queue_rx) = async_channel::bounded(nb_workers);
    let (answer_tx, answer_rx) = async_channel::bounded(nb_workers);

    // Spawn the workers
    for (i, url) in ip_list.iter().enumerate() {
        let worker = WorkerClient::new(
            i,
            url.clone(),
            queue_rx.clone(),
            answer_tx.clone(),
            partial_proof_request.clone(),
        );

        tokio::spawn(async move {
            worker.run().await;
        });
    }

    // Send the checkpoints to the workers
    for (i, checkpoint) in checkpoints.iter().enumerate() {
        queue_tx.send((i, checkpoint.clone())).await.unwrap();
    }

    let mut proofs = Vec::new();

    // Get the partial proofs from the workers
    loop {
        let (checkpoint_id, partial_proof_result) = answer_rx.recv().await.unwrap();

        match partial_proof_result {
            Ok(partial_proof) => {
                proofs.push((checkpoint_id as usize, partial_proof));
            }
            Err(_e) => {
                // Decrease the number of workers
                nb_workers -= 1;

                if nb_workers == 0 {
                    return Err(WorkerError::AllWorkersFailed);
                }

                // Push back the work for it to be done by another worker
                queue_tx
                    .send((checkpoint_id, checkpoints[checkpoint_id as usize].clone()))
                    .await
                    .unwrap();
            }
        }

        if proofs.len() == checkpoints.len() {
            break;
        }
    }

    proofs.sort_by_key(|(checkpoint_id, _)| *checkpoint_id);

    let proofs = proofs
        .into_iter()
        .map(|(_, proof)| proof)
        .flatten()
        .collect();

    Ok(proofs)
}
