use sp1_sdk::{utils, ProverClient, SP1Stdin};

const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/riscv32im-succinct-zkvm-elf-test");

#[test]
fn fibo() {
    let n = 10;
    let mut a: u128 = 0;
    let mut b: u128 = 1;
    let mut sum: u128;
    for _ in 1..n {
        sum = a + b;
        a = b;
        b = sum;
    }
}

#[test]
fn test_unittest_elf() {
    let client = ProverClient::new();
    let mut stdin = SP1Stdin::new();
    // test binary takes the same input as main binary
    stdin.write(&crate::GuestInput::default());

    let mut proof = client.prove(TEST_ELF, stdin).expect("Sp1: proving failed");

    // Verify proof.
    client
        .verify(TEST_ELF, &proof)
        .expect("Sp1: verification failed");
}
