#![cfg(feature = "enable")]
use std::env;

use alloy_primitives::B256;
use alloy_sol_types::SolValue;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverConfig, ProverResult},
};
use serde::{Deserialize, Serialize};
use sha3::{self, Digest};
use sp1_sdk::{ProverClient, SP1Stdin};

const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");
const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-guest");

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

#[test]
fn test_example() {
    // TODO(Cecilia): imple GuestInput::mock() for unit test
    // let mut client = ProverClient::new();

    // // Still need to write the same input requried by main binary
    // // Even though test itself desn't use it.
    // stdin.write(&GuestInput::default());

    // let (pk, vk) = client.setup(TEST_ELF);
    // let mut proof = client.prove(&pk, stdin).expect("Sp1: proving failed");
    // client
    //     .verify(&proof, &vk)
    //     .expect("Sp1: verification failed");
}
