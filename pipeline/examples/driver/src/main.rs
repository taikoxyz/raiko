cfg_if::cfg_if! {
    if #[cfg(feature = "sp1")] {

        use sp1_sdk::{utils, ProverClient, SP1Stdin};

        const EXAMPLE: &[u8] = include_bytes!("../../sp1/elf/example");
        const FOO: &[u8] = include_bytes!("../../sp1/elf/foo");
        const TEST_EXAMPLE: &[u8] = include_bytes!("../../sp1/elf/test-example");
        const TEST_FOO: &[u8] = include_bytes!("../../sp1/elf/test-foo");

        fn main() {
            // Setup a tracer for logging.
            utils::setup_logger();

            // Generate the proof for the given program.
            let mut client = ProverClient::new();
            [EXAMPLE, FOO]
                .iter()
                .for_each(|elf| {
                    let stdin = SP1Stdin::new();
                    let (pk, vk) = client.setup(elf);
                    let proof = client.prove(&pk, stdin).expect("proving failed");
                    client.verify(&proof, &vk).expect("verification failed");
                });

            println!("successfully generated and verified proof for the program!")
        }

        #[test]
        fn test_foo() {
            // Generate the proof for the given program.
            let mut client = ProverClient::new();
            let stdin = SP1Stdin::new();
            let (pk, vk) = cliexnt.setup(TEST_FOO);
            let proof = client.prove(&pk, stdin).expect("proving failed");
            client.verify(&proof, &vk).expect("verification failed");
        }


        #[test]
        fn test_example() {
            // Generate the proof for the given program.
            let mut client = ProverClient::new();
            let stdin = SP1Stdin::new();
            let (pk, vk) = client.setup(TEST_EXAMPLE);
            let proof = client.prove(&pk, stdin).expect("proving failed");
            client.verify(&proof, &vk).expect("verification failed");
        }

    } else if #[cfg(feature = "risc0")] {

        // use methods::example{EXAMPLE_ELF};
        use risc0_zkvm::{default_prover, ExecutorEnv, Receipt};

        fn main() {

            println!("Hello, world!");
            let env = ExecutorEnv::builder().unwrap()
            let prover = default_prover();
            // let receipt = prover.prove(env, MULTIPLY_ELF).unwrap().receipt;
        }
    }
}
