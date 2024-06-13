// Raiko
// Copyright (c) 2024 Taiko Labs
// Licensed and distributed under either of
//   * MIT license (license terms in the root directory or at http://opensource.org/licenses/MIT).
//   * Apache v2 license (license terms in the root directory or at http://www.apache.org/licenses/LICENSE-2.0).
// at your option. This file may not be copied, modified, or distributed except according to those terms.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    use raiko_lib::primitives::B256;
    use task_manager::{EnqueTaskParams, TaskDb, TaskProofsys, TaskStatus};

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

        let mut rng = ChaCha8Rng::seed_from_u64(123);

        let chain_id = 100;
        let blockhash = B256::random();
        let proof_system = TaskProofsys::Risc0;
        let submitter = "test_enqueue_task".to_owned();
        let block_number = rng.gen_range(1..4_000_000);
        let parent_hash = B256::random();
        let state_root = B256::random();
        let num_transactions = rng.gen_range(0..1000);
        let gas_used = rng.gen_range(0..100_000_000);
        let payload_length = rng.gen_range(20..200);
        let payload: Vec<u8> = (&mut rng).gen_iter::<u8>().take(payload_length).collect();

        tama.enqueue_task(EnqueTaskParams {
            chain_id,
            blockhash,
            proof_system,
            submitter,
            block_number,
            parent_hash,
            state_root,
            num_transactions,
            gas_used,
            payload,
        })
        .unwrap();
    }

    #[test]
    fn test_get_db_size() {
        // Materialized local DB
        let dir = std::env::current_dir().unwrap().join("tests");
        let file = dir.as_path().join("test_get_db_size.sqlite");
        if file.exists() {
            std::fs::remove_file(&file).unwrap()
        };

        // // temp dir DB
        // use tempfile::tempdir;
        // let dir = tempdir().unwrap();
        // let file = dir.path().join("test_get_db_size.sqlite");

        #[allow(unused_mut)]
        let mut db = TaskDb::open_or_create(&file).unwrap();
        // db.set_tracer(Some(|stmt| println!("sqlite:\n-------\n{}\n=======", stmt)));
        let mut tama = db.manage().unwrap();

        let mut rng = ChaCha8Rng::seed_from_u64(123);

        for _ in 0..42 {
            let chain_id = 100;
            let blockhash = B256::random();
            let proof_system = TaskProofsys::Risc0;
            let submitter = format!("test_get_db_size/{}", rng.gen_range(1..10));
            let block_number = rng.gen_range(1..4_000_000);
            let parent_hash = B256::random();
            let state_root = B256::random();
            let num_transactions = rng.gen_range(0..1000);
            let gas_used = rng.gen_range(0..100_000_000);
            let payload_length = rng.gen_range(1_000_000..10_000_000);
            let payload: Vec<u8> = (&mut rng).gen_iter::<u8>().take(payload_length).collect();

            tama.enqueue_task(EnqueTaskParams {
                chain_id,
                blockhash,
                proof_system,
                submitter,
                block_number,
                parent_hash,
                state_root,
                num_transactions,
                gas_used,
                payload,
            })
            .unwrap();
        }

        let (db_size, db_tables_size) = tama.get_db_size().unwrap();
        println!("db_tables_size: {:?}", db_tables_size);
        assert!(db_size / 1024 / 1024 > 40);
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
            let chain_id = 100;
            let blockhash = B256::random();
            let proof_system = TaskProofsys::Risc0;
            let submitter = format!("test_get_db_size/{}", rng.gen_range(1..10));
            let block_number = rng.gen_range(1..4_000_000);
            let parent_hash = B256::random();
            let state_root = B256::random();
            let num_transactions = rng.gen_range(0..1000);
            let gas_used = rng.gen_range(0..100_000_000);
            let payload_length = rng.gen_range(16..64);
            let payload: Vec<u8> = (&mut rng).gen_iter::<u8>().take(payload_length).collect();

            tama.enqueue_task(EnqueTaskParams {
                chain_id,
                blockhash,
                proof_system,
                submitter: submitter.clone(),
                block_number,
                parent_hash,
                state_root,
                num_transactions,
                gas_used,
                payload,
            })
            .unwrap();

            let task_status = tama
                .get_task_proving_status(chain_id, &blockhash, proof_system)
                .unwrap();
            assert_eq!(task_status.len(), 1);
            assert_eq!(task_status[0].0, Some(submitter.clone()));
            assert_eq!(task_status[0].1, TaskStatus::Registered);

            tasks.push((chain_id, blockhash, proof_system, submitter));
        }

        std::thread::sleep(Duration::from_millis(1));

        {
            tama.update_task_progress(
                tasks[0].0,
                &tasks[0].1,
                tasks[0].2,
                None,
                TaskStatus::Cancelled_NeverStarted,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[0].0, &tasks[0].1, tasks[0].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, None);
                assert_eq!(task_status[0].1, TaskStatus::Cancelled_NeverStarted);
                assert_eq!(task_status[1].0, Some(tasks[0].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }
        }
        // -----------------------
        {
            tama.update_task_progress(
                tasks[1].0,
                &tasks[1].1,
                tasks[1].2,
                Some("A prover Network"),
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[1].0, &tasks[1].1, tasks[1].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, Some(String::from("A prover Network")));
                assert_eq!(task_status[0].1, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, Some(tasks[1].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[1].0,
                &tasks[1].1,
                tasks[1].2,
                Some("A prover Network"),
                TaskStatus::CancellationInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[1].0, &tasks[1].1, tasks[1].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, Some(String::from("A prover Network")));
                assert_eq!(task_status[0].1, TaskStatus::CancellationInProgress);
                assert_eq!(task_status[1].0, Some(tasks[1].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[1].0,
                &tasks[1].1,
                tasks[1].2,
                Some("A prover Network"),
                TaskStatus::Cancelled,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[1].0, &tasks[1].1, tasks[1].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, Some(String::from("A prover Network")));
                assert_eq!(task_status[0].1, TaskStatus::Cancelled);
                assert_eq!(task_status[1].0, Some(tasks[1].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }
        }

        // -----------------------
        {
            tama.update_task_progress(
                tasks[2].0,
                &tasks[2].1,
                tasks[2].2,
                Some("A based prover"),
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[2].0, &tasks[2].1, tasks[2].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, Some(String::from("A based prover")));
                assert_eq!(task_status[0].1, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, Some(tasks[2].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            let proof: Vec<_> = (&mut rng).gen_iter::<u8>().take(128).collect();
            tama.update_task_progress(
                tasks[2].0,
                &tasks[2].1,
                tasks[2].2,
                Some("A based prover"),
                TaskStatus::Success,
                Some(&proof),
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[2].0, &tasks[2].1, tasks[2].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, Some(String::from("A based prover")));
                assert_eq!(task_status[0].1, TaskStatus::Success);
                assert_eq!(task_status[1].0, Some(tasks[2].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }

            assert_eq!(
                proof,
                tama.get_task_proof(tasks[2].0, &tasks[2].1, tasks[2].2)
                    .unwrap()
            );
        }

        // -----------------------
        {
            tama.update_task_progress(
                tasks[3].0,
                &tasks[3].1,
                tasks[3].2,
                Some("A flaky prover"),
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, &tasks[3].1, tasks[3].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, Some(String::from("A flaky prover")));
                assert_eq!(task_status[0].1, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, Some(tasks[3].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[3].0,
                &tasks[3].1,
                tasks[3].2,
                Some("A flaky prover"),
                TaskStatus::NetworkFailure,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, &tasks[3].1, tasks[3].2)
                    .unwrap();
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[0].0, Some(String::from("A flaky prover")));
                assert_eq!(task_status[0].1, TaskStatus::NetworkFailure);
                assert_eq!(task_status[1].0, Some(tasks[3].3.clone()));
                assert_eq!(task_status[1].1, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                tasks[3].0,
                &tasks[3].1,
                tasks[3].2,
                Some("A based prover"),
                TaskStatus::WorkInProgress,
                None,
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, &tasks[3].1, tasks[3].2)
                    .unwrap();
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[0].0, Some(String::from("A based prover")));
                assert_eq!(task_status[0].1, TaskStatus::WorkInProgress);
                assert_eq!(task_status[1].0, Some(String::from("A flaky prover")));
                assert_eq!(task_status[1].1, TaskStatus::NetworkFailure);
                assert_eq!(task_status[2].0, Some(tasks[3].3.clone()));
                assert_eq!(task_status[2].1, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            let proof: Vec<_> = (&mut rng).gen_iter::<u8>().take(128).collect();
            tama.update_task_progress(
                tasks[3].0,
                &tasks[3].1,
                tasks[3].2,
                Some("A based prover"),
                TaskStatus::Success,
                Some(&proof),
            )
            .unwrap();

            {
                let task_status = tama
                    .get_task_proving_status(tasks[3].0, &tasks[3].1, tasks[3].2)
                    .unwrap();
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[0].0, Some(String::from("A based prover")));
                assert_eq!(task_status[0].1, TaskStatus::Success);
                assert_eq!(task_status[1].0, Some(String::from("A flaky prover")));
                assert_eq!(task_status[1].1, TaskStatus::NetworkFailure);
                assert_eq!(task_status[2].0, Some(tasks[3].3.clone()));
                assert_eq!(task_status[2].1, TaskStatus::Registered);
            }

            assert_eq!(
                proof,
                tama.get_task_proof(tasks[3].0, &tasks[3].1, tasks[3].2)
                    .unwrap()
            );
        }
    }
}
