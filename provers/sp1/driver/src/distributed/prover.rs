use std::env;

use super::worker::Worker;
use alloy_primitives::B256;
use alloy_sol_types::SolValue;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverConfig, ProverResult},
};
use sha3::{self, Digest};
use sp1_sdk::{ProverClient, SP1Stdin};

use crate::{Sp1Response, ELF};

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
            return Self::run_as_worker(input, output, &config).await;
        }

        return Self::run_as_orchestrator(input, output, &config).await;
    }

    fn instance_hash(pi: ProtocolInstance) -> B256 {
        let data = (pi.transition.clone(), pi.prover, pi.meta_hash()).abi_encode();

        let hash: [u8; 32] = sha3::Keccak256::digest(data).into();
        hash.into()
    }
}

impl Sp1DistributedProver {
    pub async fn run_as_orchestrator(
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let now = std::time::Instant::now();

        log::info!("Running SP1 Distributed orchestrator");

        // Write the input.
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = ProverClient::new();
        let (_pk, vk) = client.setup(ELF);

        // Execute the program to get the public values and the number of checkpoints
        let (nb_checkpoint, public_values) = client
            .nb_checkpoints(ELF, stdin.clone())
            .expect("Sp1: execution failed");

        log::info!("Number of checkpoints: {}", nb_checkpoint);

        let ip_list = std::fs::read_to_string("distributed.json").unwrap();
        let ip_list: Vec<String> = serde_json::from_str(&ip_list).unwrap();

        let (queue_tx, queue_rx) = async_channel::unbounded();
        let (answer_tx, answer_rx) = async_channel::unbounded();

        // Spawn the workers
        for (i, url) in ip_list.iter().enumerate() {
            let worker = Worker::new(
                i,
                "http://".to_string() + url + "/proof".into(),
                config.clone(),
                queue_rx.clone(),
                answer_tx.clone(),
                queue_tx.clone(),
            );

            tokio::spawn(async move {
                worker.run().await;
            });
        }

        // Send the checkpoints to the workers
        for i in 0..nb_checkpoint {
            queue_tx.send(i).await.unwrap();
        }

        let mut proofs = Vec::new();

        // Get the partial proofs from the workers
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

    pub async fn run_as_worker(
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
        let (pk, _vk) = client.setup(ELF);

        let partial_proof = client
            .prove_partial(&pk, stdin.clone(), checkpoint)
            .expect("Sp1: proving failed");

        to_proof(Ok(Sp1Response {
            proof: serde_json::to_string(&partial_proof).unwrap(),
            output: output.clone(),
        }))
    }
}
