// Raiko
// Copyright (c) 2024 Taiko Labs
// Licensed and distributed under either of
//   * MIT license (license terms in the root directory or at http://opensource.org/licenses/MIT).
//   * Apache v2 license (license terms in the root directory or at http://www.apache.org/licenses/LICENSE-2.0).
// at your option. This file may not be copied, modified, or distributed except according to those terms.

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use alloy_primitives::Address;
    use raiko_core::interfaces::{ProofRequest, ProofType};
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    use raiko_lib::primitives::B256;
    use raiko_task_manager::{TaskDb, TaskStatus};

    fn create_random_task(rng: &mut ChaCha8Rng) -> (u64, B256, ProofRequest) {
        let chain_id = 100;
        let proof_type = match rng.gen_range(0..4) {
            0 => ProofType::Native,
            1 => ProofType::Sgx,
            2 => ProofType::Sp1,
            _ => ProofType::Risc0,
        };
        let block_number = rng.gen_range(1..4_000_000);
        let block_hash = B256::random();
        let graffiti = B256::random();
        let prover_args = HashMap::new();
        let prover = Address::random();

        (
            chain_id,
            block_hash,
            ProofRequest {
                block_number,
                network: "network".to_string(),
                l1_network: "l1_network".to_string(),
                graffiti,
                prover,
                proof_type,
                prover_args,
            },
        )
    }

    #[test]
    fn test_enqueue_task() {
        // // Materialized local DB
        // let dir = std::env::current_dir().unwrap().join("tests");
        // let file = dir.as_path().join("test_enqueue_task.sqlite");
        // if file.exists() {
        //     std::fs::remove_file(&file).unwrap()
        // };

        // temp dir DB
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let file = dir.path().join("test_enqueue_task.sqlite");

        #[allow(unused_mut)]
        let mut db = TaskDb::open_or_create(&file).unwrap();
        // db.set_tracer(Some(|stmt| println!("sqlite:\n-------\n{}\n=======", stmt)));
        let mut tama = db.manage().unwrap();

        let (chain_id, block_hash, request) =
            create_random_task(&mut ChaCha8Rng::seed_from_u64(123));
        tama.enqueue_task(chain_id, block_hash, &request).unwrap();
    }

    #[test]
    fn test_update_query_tasks_progress() {
        // Materialized local DB
        let dir = std::env::current_dir().unwrap().join("tests");
        let file = dir
            .as_path()
            .join("test_update_query_tasks_progress.sqlite");
        if file.exists() {
            std::fs::remove_file(&file).unwrap()
        };

        // // temp dir DB
        // use tempfile::tempdir;
        // let dir = tempdir().unwrap();
        // let file = dir.path().join("test_update_task_progress.sqlite");

        #[allow(unused_mut)]
        let mut db = TaskDb::open_or_create(&file).unwrap();
        // db.set_tracer(Some(|stmt| println!("sqlite:\n-------\n{}\n=======", stmt)));
        let mut tama = db.manage().unwrap();

        let mut rng = ChaCha8Rng::seed_from_u64(123);
        let mut tasks = vec![];

        for _ in 0..5 {
            let (chain_id, block_hash, request) = create_random_task(&mut rng);

            tama.enqueue_task(chain_id, block_hash, &request).unwrap();

            let task_status = tama
                .get_task_proving_status(chain_id, block_hash, request.proof_type)
                .unwrap();
            assert_eq!(task_status.len(), 1);
            let (status, _) = task_status
                .first()
                .expect("Already confirmed there is exactly 1 element");
            assert_eq!(status, &TaskStatus::Registered);

            tasks.push((
                chain_id,
                block_hash,
                request.block_number,
                request.proof_type,
            ));
        }

        std::thread::sleep(Duration::from_millis(1));

        {
            let task_status = tama
                .get_task_proving_status(tasks[0].0, tasks[0].1, tasks[0].3)
                .unwrap();
            println!("{task_status:?}");
            tama.update_task_progress(
                tasks[0].0,
                tasks[0].1,
                tasks[0].3,
                TaskStatus::Cancelled_NeverStarted,
                None,
            )
            .unwrap();

            let task_status = tama
                .get_task_proving_status(tasks[0].0, tasks[0].1, tasks[0].3)
                .unwrap();
            println!("{task_status:?}");
            assert_eq!(task_status.len(), 2);
            assert_eq!(task_status[0].0, TaskStatus::Cancelled_NeverStarted);
            assert_eq!(task_status[1].0, TaskStatus::Registered);
        }
        // -----------------------
        {
            tama.update_task_progress(
                tasks[1].0,
                tasks[1].1,
                tasks[1].3,
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[1].0, tasks[1].1, tasks[1].3)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[1].0,
                tasks[1].1,
                tasks[1].3,
                TaskStatus::CancellationInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[1].0, tasks[1].1, tasks[1].3)
                    .unwrap();
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[0].0, TaskStatus::CancellationInProgress);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[2].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[1].0,
                tasks[1].1,
                tasks[1].3,
                TaskStatus::Cancelled,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[1].0, tasks[1].1, tasks[1].3)
                    .unwrap();
                assert_eq!(task_status.len(), 4);
                assert_eq!(task_status[0].0, TaskStatus::Cancelled);
                assert_eq!(task_status[1].0, TaskStatus::CancellationInProgress);
                assert_eq!(task_status[2].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[3].0, TaskStatus::Registered);
            }
        }

        // -----------------------
        {
            tama.update_task_progress(
                tasks[2].0,
                tasks[2].1,
                tasks[2].3,
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[2].0, tasks[2].1, tasks[2].3)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            let proof: Vec<_> = (&mut rng).gen_iter::<u8>().take(128).collect();
            tama.update_task_progress(
                tasks[2].0,
                tasks[2].1,
                tasks[2].3,
                TaskStatus::Success,
                Some(&proof),
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[2].0, tasks[2].1, tasks[2].3)
                    .unwrap();
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[0].0, TaskStatus::Success);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[2].0, TaskStatus::Registered);
            }

            assert_eq!(
                proof,
                tama.get_task_proof(tasks[2].0, tasks[2].1, tasks[2].3)
                    .unwrap()
            );
        }

        // -----------------------
        {
            tama.update_task_progress(
                tasks[3].0,
                tasks[3].1,
                tasks[3].3,
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, tasks[3].1, tasks[3].3)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[3].0,
                tasks[3].1,
                tasks[3].3,
                TaskStatus::NetworkFailure,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, tasks[3].1, tasks[3].3)
                    .unwrap();
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[0].0, TaskStatus::NetworkFailure);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[2].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[3].0,
                tasks[3].1,
                tasks[3].3,
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, tasks[3].1, tasks[3].3)
                    .unwrap();
                assert_eq!(task_status.len(), 4);
                assert_eq!(task_status[0].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, TaskStatus::NetworkFailure);
                assert_eq!(task_status[2].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[3].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            let proof: Vec<_> = (&mut rng).gen_iter::<u8>().take(128).collect();
            tama.update_task_progress(
                tasks[3].0,
                tasks[3].1,
                tasks[3].3,
                TaskStatus::Success,
                Some(&proof),
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, tasks[3].1, tasks[3].3)
                    .unwrap();
                assert_eq!(task_status.len(), 5);
                assert_eq!(task_status[0].0, TaskStatus::Success);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[2].0, TaskStatus::NetworkFailure);
                assert_eq!(task_status[3].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[4].0, TaskStatus::Registered);
            }

            assert_eq!(
                proof,
                tama.get_task_proof(tasks[3].0, tasks[3].1, tasks[3].3)
                    .unwrap()
            );
        }
    }
}
