#![cfg(feature = "enable")]
use std::env;

use alloy_primitives::B256;
use alloy_sol_types::SolValue;
use async_channel::{Receiver, Sender};
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverConfig, ProverResult},
};
use serde::{Deserialize, Serialize};
use sha3::{self, Digest};
use sp1_sdk::{CoreSC, ProverClient, SP1CoreProof, SP1PublicValues, SP1Stdin};

const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: String,
    pub output: GuestOutput,
}

pub struct Sp1Prover;

impl Prover for Sp1Prover {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
    ) -> ProverResult<Proof> {
        // Write the input.
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = ProverClient::new();
        let (pk, vk) = client.setup(ELF);
        let mut proof = client.prove(&pk, stdin).expect("Sp1: proving failed");

        // Read the output.
        let output = proof.public_values.read::<GuestOutput>();
        // Verify proof.
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");

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

        println!("succesfully generated and verified proof for the program!");
        to_proof(Ok(Sp1Response {
            proof: serde_json::to_string(&proof).unwrap(),
            output,
        }))
    }

    fn instance_hash(pi: ProtocolInstance) -> B256 {
        let data = (pi.transition.clone(), pi.prover, pi.meta_hash()).abi_encode();

        let hash: [u8; 32] = sha3::Keccak256::digest(data).into();
        hash.into()
    }
}

pub struct Sp1DistributedProver;

impl Prover for Sp1DistributedProver {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        println!("Running SP1 Distributed prover");

        let mut config = config.clone();

        // Fixing the network and proof type to be forwarded to the workers
        let mut_config = config.as_object_mut().unwrap();
        mut_config.insert("network".to_string(), "taiko_a7".into());
        mut_config.insert("proof_type".to_string(), "sp1_distributed".into());

        if config.get("sp1").map(|sp1| sp1.get("checkpoint")).is_some() {
            return Self::worker(input, output, &config).await;
        }

        return Self::orchestrator(input, output, &config).await;
    }

    fn instance_hash(pi: ProtocolInstance) -> B256 {
        let data = (pi.transition.clone(), pi.prover, pi.meta_hash()).abi_encode();

        let hash: [u8; 32] = sha3::Keccak256::digest(data).into();
        hash.into()
    }
}

struct Worker {
    id: usize,
    // The url of the worker
    url: String,
    // The config to send to the worker
    config: ProverConfig,
    // A queue to receive the checkpoint to compute the partial proof
    queue: Receiver<usize>,
    // A channel to send back the id of the checkpoint along with the json strings encoding the computed partial proofs
    answer: Sender<(usize, String)>,
    // if an error occured, send the checkpoint back in the queue for another worker to pick it up
    queue_push_back: Sender<usize>,
}

impl Worker {
    pub fn new(
        id: usize,
        url: String,
        config: ProverConfig,
        queue: Receiver<usize>,
        answer: Sender<(usize, String)>,
        queue_push_back: Sender<usize>,
    ) -> Self {
        Worker {
            id,
            url,
            config,
            queue,
            answer,
            queue_push_back,
        }
    }

    pub async fn run(&self) {
        while let Ok(checkpoint) = self.queue.recv().await {
            // Compute the partial proof
            let partial_proof_result = self.send_work(checkpoint).await;

            match partial_proof_result {
                Ok(partial_proof) => self.answer.send((checkpoint, partial_proof)).await.unwrap(),
                Err(e) => {
                    self.queue_push_back.send(checkpoint).await.unwrap();

                    break;
                }
            }
        }
    }

    async fn send_work(&self, checkpoint: usize) -> Result<String, reqwest::Error> {
        log::info!(
            "Sending checkpoint {} to worker {}: {}",
            checkpoint,
            self.id,
            self.url
        );

        let mut config = self.config.clone();

        let mut_config = config.as_object_mut().unwrap();
        mut_config.insert(
            "sp1".to_string(),
            serde_json::json!({
                "checkpoint": checkpoint,
            }),
        );

        let now = std::time::Instant::now();

        let response_result = reqwest::Client::new()
            .post(&self.url)
            .json(&config)
            .send()
            .await;

        log::info!(
            "Received proof for checkpoint {} from worker {}: {} in {}s",
            checkpoint,
            self.id,
            self.url,
            now.elapsed().as_secs()
        );

        match response_result {
            Ok(response) => {
                let sp1_response: Sp1Response = response.json().await.unwrap();

                Ok(sp1_response.proof)
            }
            Err(e) => Err(e),
        }
    }
}

impl Sp1DistributedProver {
    pub async fn orchestrator(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let now = std::time::Instant::now();

        log::info!("Running SP1 Distributed orchestrator");

        // Write the input.
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = ProverClient::new();
        let (pk, vk) = client.setup(ELF);

        let (nb_checkpoint, public_values) = client
            .nb_checkpoints(ELF, stdin.clone())
            .expect("Sp1: execution failed");

        log::info!("Number of checkpoints: {}", nb_checkpoint);

        let ip_list = std::fs::read_to_string("distributed.json").unwrap();
        let ip_list: Vec<String> = serde_json::from_str(&ip_list).unwrap();

        let (queue_tx, queue_rx) = async_channel::unbounded();
        let (answer_tx, answer_rx) = async_channel::unbounded();

        for (i, url) in ip_list.iter().enumerate() {
            let worker = Worker::new(
                i,
                url.clone(),
                config.clone(),
                queue_rx.clone(),
                answer_tx.clone(),
                queue_tx.clone(),
            );

            tokio::spawn(async move {
                worker.run().await;
            });
        }

        for i in 0..nb_checkpoint {
            queue_tx.send(i).await.unwrap();
        }

        let mut proofs = Vec::new();

        loop {
            let (checkpoint_id, partial_proof_json) = answer_rx.recv().await.unwrap();

            let partial_proof =
                serde_json::from_str::<Vec<_>>(partial_proof_json.as_str()).unwrap();

            proofs.push((checkpoint_id, partial_proof));

            if proofs.len() == nb_checkpoint {
                break;
            }
        }

        proofs.sort_by_key(|(checkpoint_id, _)| *checkpoint_id);

        let proofs = proofs
            .into_iter()
            .map(|(_, proof)| proof)
            .flatten()
            .collect();

        /* let pool = Pool::bounded(ip_list.len());

        let mut futures = Vec::new();

        for i in 0..nb_checkpoint {
            let url = "http://".to_owned() + &ip_list[i % ip_list.len()] + "/proof";
            let config = config.clone();
            let input = input.clone();
            let output = output.clone();
            let url = url.clone();
            let ip_list = ip_list.clone();

            let partial_proof = pool
                .spawn(async move {
                    log::info!(
                        "Sending checkpoint {}/{} to worker {}: {}",
                        i + 1,
                        nb_checkpoint,
                        i % ip_list.len() + 1,
                        url
                    );

                    let mut config = config.clone();

                    let mut_config = config.as_object_mut().unwrap();
                    mut_config.insert(
                        "sp1".to_string(),
                        serde_json::json!({
                            "checkpoint": i,
                        }),
                    );

                    let now = std::time::Instant::now();

                    let response = reqwest::Client::new()
                        .post(url.clone())
                        .json(&config)
                        .send()
                        .await
                        .expect("Sp1: proving shard failed");

                    let sp1_response: Sp1Response = response.json().await.unwrap();

                    log::info!(
                        "Received proof shard {}/{} from worker {}: {} in {}s",
                        i + 1,
                        nb_checkpoint,
                        i % ip_list.len() + 1,
                        url,
                        now.elapsed().as_secs()
                    );

                    sp1_response.proof
                })
                .await
                .unwrap();

            futures.push(partial_proof);
        }

        let mut proofs = Vec::new();

        for future in futures {
            let partial_proof_json = future.await.unwrap().unwrap();

            let partial_proof =
                serde_json::from_str::<Vec<_>>(partial_proof_json.as_str()).unwrap();

            proofs.extend(partial_proof);
        } */

        let mut proof = sp1_sdk::SP1ProofWithPublicValues {
            proof: proofs,
            stdin: stdin.clone(),
            public_values,
        };

        // Read the output.
        let output = proof.public_values.read::<GuestOutput>();

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
            output,
        }))
    }

    pub async fn worker(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let checkpoint = config
            .get("sp1")
            .unwrap()
            .as_object()
            .unwrap()
            .get("checkpoint")
            .unwrap()
            .as_u64()
            .unwrap() as usize;

        println!("Running SP1 Distributed worker {}", checkpoint);

        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = ProverClient::new();
        let (pk, vk) = client.setup(ELF);

        let (partial_proof, public_values) = client
            .prove_partial(&pk, stdin.clone(), checkpoint)
            .expect("Sp1: proving failed");

        to_proof(Ok(Sp1Response {
            proof: serde_json::to_string(&partial_proof).unwrap(),
            output: output.clone(),
        }))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-guest");

    #[test]
    fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::new();
        let stdin = SP1Stdin::new();
        let (pk, vk) = client.setup(TEST_ELF);
        let proof = client.prove(&pk, stdin).expect("Sp1: proving failed");
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");
    }
}
