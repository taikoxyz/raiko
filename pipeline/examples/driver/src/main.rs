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

            println!("successfully generated and verified proof for the program!");
        }

        #[test]
        #[should_panic]
        fn test_foo() {
            // Generate the proof for the given program.
            let mut client = ProverClient::new();
            let stdin = SP1Stdin::new();
            let (pk, vk) = client.setup(TEST_FOO);
            let proof = client.prove(&pk, stdin).expect("proving failed");
            client.verify(&proof, &vk).expect("verification failed");
        }


        #[test]
        #[should_panic]
        fn test_example() {
            // Generate the proof for the given program.
            let mut client = ProverClient::new();
            let stdin = SP1Stdin::new();
            let (pk, vk) = client.setup(TEST_EXAMPLE);
            let proof = client.prove(&pk, stdin).expect("proving failed");
            client.verify(&proof, &vk).expect("verification failed");
        }

    } else if #[cfg(feature = "risc0")] {

        use methods::{
            example::{EXAMPLE_ELF, EXAMPLE_ID},
            foo::{FOO_ELF, FOO_ID},
            test_foo::{TEST_FOO_ELF, TEST_FOO_ID},
            test_bar::{TEST_BAR_ELF, TEST_BAR_ID},
        };
        use risc0_zkvm::{default_prover, ExecutorEnv};


        fn main() {
            [EXAMPLE_ELF, FOO_ELF]
                .iter()
                .zip([EXAMPLE_ID, FOO_ID].iter())
                .for_each(|(elf, id)| {
                    let env = ExecutorEnv::builder().build().unwrap();
                    let prover = default_prover();
                    let receipt = prover
                        .prove(env, elf)
                        .unwrap();
                    receipt
                        .verify(*id)
                        .unwrap();
                });
            println!("successfully generated and verified proof for the program!");
        }

        #[test]
        #[should_panic]
        fn test_foo() {
            let env = ExecutorEnv::builder().build().unwrap();
            let prover = default_prover();
            let receipt = prover
                .prove(env, TEST_FOO_ELF)
                .unwrap();
            receipt
                .verify(TEST_FOO_ID)
                .unwrap();
        }

        #[test]
        #[should_panic]
        fn test_bar() {
            let env = ExecutorEnv::builder().build().unwrap();
            let prover = default_prover();
            let receipt = prover
                .prove(env, TEST_BAR_ELF)
                .unwrap();
            receipt
                .verify(TEST_BAR_ID)
                .unwrap();

        }
    } else {
        fn main() {
            println!("Please use --features to specify the target platform.");
        }
    }
}
