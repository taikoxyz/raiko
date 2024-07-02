use std::env;

use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{to_proof, Proof, Prover, ProverConfig, ProverResult, WorkerError},
};
use sp1_core::{runtime::Program, stark::ShardProof, utils::BabyBearPoseidon2};
use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1Stdin};

use crate::{
    distributed::{
        partial_proof_request::PartialProofRequest,
        sp1_specifics::{commit, prove_partial},
    },
    Sp1Response, WorkerProtocol, WorkerSocket, ELF,
};

pub struct Sp1DistributedProver;

impl Prover for Sp1DistributedProver {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
    ) -> ProverResult<Proof> {
        log::info!("Running SP1 Distributed orchestrator");

        let now = std::time::Instant::now();

        // Write the input.
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = ProverClient::new();
        let (_pk, vk) = client.setup(ELF);

        let proof = Self::run_as_orchestrator(stdin).await?;

        // Verify proof.
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");

        log::info!(
            "Proof generation and verification took: {:?}s",
            now.elapsed().as_secs()
        );

        // Save the proof.
        let proof_dir = env::current_dir().expect("Sp1: dir error");

        proof
            .save(
                proof_dir
                    .as_path()
                    .join("proof-with-io.json")
                    .to_str()
                    .unwrap(),
            )
            .expect("Sp1: saving proof failed");

        to_proof(Ok(Sp1Response {
            proof: serde_json::to_string(&proof).unwrap(),
        }))
    }
}

impl Sp1DistributedProver {
    pub async fn run_as_orchestrator(
        stdin: SP1Stdin,
    ) -> ProverResult<SP1ProofWithPublicValues<Vec<ShardProof<BabyBearPoseidon2>>>> {
        let program = Program::from(ELF);

        let worker_ip_list = Self::read_and_validate_worker_ip_list().await?;

        let (checkpoints, public_values_stream, partial_proof_request) =
            commit(program, &stdin, worker_ip_list.len()).unwrap();

        let proofs = super::orchestrator::distribute_work(
            worker_ip_list,
            checkpoints,
            partial_proof_request,
        )
        .await?;

        Ok(SP1ProofWithPublicValues {
            proof: proofs,
            stdin,
            public_values: public_values_stream,
            sp1_version: sp1_core::SP1_CIRCUIT_VERSION.to_string(),
        })
    }

    pub async fn run_as_worker(
        partial_proof_request: PartialProofRequest,
    ) -> ProverResult<Vec<ShardProof<BabyBearPoseidon2>>> {
        log::debug!(
            "Running SP1 Distributed worker: Prove shard nb {}",
            partial_proof_request.checkpoint_id
        );

        let partial_proof = prove_partial(&partial_proof_request);

        Ok(partial_proof)
    }

    async fn read_and_validate_worker_ip_list() -> Result<Vec<String>, WorkerError> {
        let ip_list_string = std::fs::read_to_string("distributed.json")
            .expect("Sp1 Distributed: Need a `distributed.json` file with a list of IP:PORT");

        let ip_list: Vec<String> = serde_json::from_str(&ip_list_string).expect(
            "Sp1 Distributed: Invalid JSON for `distributed.json`. need an array of IP:PORT",
        );

        let mut reachable_ip_list = Vec::new();

        // try to connect to each worker to make sure they are reachable
        for ip in &ip_list {
            let Ok(mut socket) = WorkerSocket::connect(ip).await else {
                log::warn!("Sp1 Distributed: Worker at {} is not reachable. Removing from the list for this task", ip);

                continue;
            };

            if let Err(_) = socket.send(WorkerProtocol::Ping).await {
                log::warn!("Sp1 Distributed: Worker at {} is not reachable. Removing from the list for this task", ip);

                continue;
            }

            let Ok(response) = socket.receive().await else {
                log::warn!("Sp1 Distributed: Worker at {} is not a valid SP1 worker. Removing from the list for this task", ip);

                continue;
            };

            if let WorkerProtocol::Ping = response {
                reachable_ip_list.push(ip.clone());
            } else {
                log::warn!("Sp1 Distributed: Worker at {} is not a valid SP1 worker. Removing from the list for this task", ip);
            }
        }

        if reachable_ip_list.is_empty() {
            log::error!("Sp1 Distributed: No reachable workers found. Aborting...");

            return Err(WorkerError::AllWorkersFailed);
        }

        Ok(reachable_ip_list)
    }
}
