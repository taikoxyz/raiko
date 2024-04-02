#![cfg(feature = "enable")]

use std::env;

use raiko_lib::input::{GuestInput, GuestOutput};
use serde::{Deserialize, Serialize};
use sp1_core::{utils, SP1Prover, SP1Stdin, SP1Verifier};

const ELF: &[u8] = include_bytes!("../../guest/elf/riscv32im-succinct-zkvm-elf");

pub async fn execute(input: GuestInput) -> Result<Sp1Response, String> {
    let config = utils::BabyBearBlake3::new();

    // Write the input.
    let mut stdin = SP1Stdin::new();
    stdin.write(&input);

    // Generate the proof for the given program.
    let mut proof =
        SP1Prover::prove_with_config(ELF, stdin, config.clone()).expect("Sp1: proving failed");

    // Read the output.
    let output = proof.stdout.read::<GuestOutput>();

    // Verify proof.
    SP1Verifier::verify_with_config(ELF, &proof, config).expect("Sp1: verification failed");

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
    Ok(Sp1Response {
        proof: serde_json::to_string(&proof).unwrap(),
        output,
    })
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: String,
    pub output: GuestOutput,
}
