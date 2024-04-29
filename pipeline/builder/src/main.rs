cfg_if::cfg_if! {
    if #[cfg(feature = "sp1")] {

        use sp1_sdk::{utils, ProverClient, SP1Stdin};

        const EXAMPLE: &[u8] = include_bytes!("../../example-sp1/elf/example");
        const FOO: &[u8] = include_bytes!("../../example-sp1/elf/foo");
        const TEST_BAR: &[u8] = include_bytes!("../../example-sp1/elf/test-bar");
        const TEST_EXAMPLE: &[u8] = include_bytes!("../../example-sp1/elf/test-example");

        fn main() {
            // Setup a tracer for logging.
            utils::setup_logger();

            // Generate the proof for the given program.
            let mut client = ProverClient::new();
            [/* EXAMPLE, FOO, TEST_BAR, */ TEST_EXAMPLE ]
                .iter()
                .for_each(|elf| {
                    let stdin = SP1Stdin::new();
                    let (pk, vk) = client.setup(elf);
                    let proof = client.prove(&pk, stdin).expect("proving failed");
                    client.verify(&proof, &vk).expect("verification failed");
                    // proof.save("proof-with-pis.json").expect("saving proof failed");
                });

            println!("successfully generated and verified proof for the program!")
        }

    } else {
        fn main() {
            println!("Hello, world!");
        }
    }
}
