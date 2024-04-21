use sp1_sdk::{utils, ProverClient, SP1Stdin};


const ELF: &[u8] = include_bytes!("../../guest/elf/riscv32im-succinct-zkvm-elf-test");

#[test]
fn test_unittest_elf() {
    // Generate the proof for the given program.
    let client = ProverClient::new();
    let mut stdin = SP1Stdin::new();
    let mut proof = client.prove(ELF, stdin).expect("Sp1: proving failed");

    // Verify proof.
    client
        .verify(ELF, &proof)
        .expect("Sp1: verification failed");
}
