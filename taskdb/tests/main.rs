// Raiko
// Copyright (c) 2024 Taiko Labs
// Licensed and distributed under either of
//   * MIT license (license terms in the root directory or at http://opensource.org/licenses/MIT).
//   * Apache v2 license (license terms in the root directory or at http://www.apache.org/licenses/LICENSE-2.0).
// at your option. This file may not be copied, modified, or distributed except according to those terms.

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, env, time::Duration};

    use alloy_primitives::Address;
    use raiko_core::interfaces::{ProofRequest, ProofType};
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    use raiko_lib::{input::BlobProofType, primitives::B256};
    use raiko_tasks::{
        get_task_manager, ProofTaskDescriptor, TaskManager, TaskManagerOpts, TaskStatus,
    };

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
                blob_proof_type: BlobProofType::ProofOfEquivalence,
                l1_inclusion_block_number: 0,
            },
        )
    }

    #[tokio::test]
    async fn test_enqueue_task() {
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

        let mut tama = get_task_manager(&TaskManagerOpts {
            sqlite_file: file,
            max_db_size: 1_000_000,
            redis_url: env::var("REDIS_URL").unwrap_or_default(),
        });

        let (chain_id, blockhash, request) =
            create_random_task(&mut ChaCha8Rng::seed_from_u64(123));
        tama.enqueue_task(
            &(
                chain_id,
                request.block_number,
                blockhash,
                request.proof_type,
                request.prover.to_string(),
            )
                .into(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_update_query_tasks_progress() {
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

        let mut tama = get_task_manager(&TaskManagerOpts {
            sqlite_file: file,
            max_db_size: 1_000_000,
            redis_url: env::var("REDIS_URL").unwrap_or_default(),
        });

        let mut rng = ChaCha8Rng::seed_from_u64(123);
        let mut tasks = vec![];

        for _ in 0..5 {
            let (chain_id, blockhash, request) = create_random_task(&mut rng);

            tama.enqueue_task(
                &(
                    chain_id,
                    request.block_number,
                    blockhash,
                    request.proof_type,
                    request.prover.to_string(),
                )
                    .into(),
            )
            .await
            .unwrap();

            let task_status = tama
                .get_task_proving_status(
                    &(
                        chain_id,
                        request.block_number,
                        blockhash,
                        request.proof_type,
                        request.prover.to_string(),
                    )
                        .into(),
                )
                .await
                .unwrap()
                .0;
            assert_eq!(task_status.len(), 1);
            let status = task_status
                .first()
                .expect("Already confirmed there is exactly 1 element");
            assert_eq!(status.0, TaskStatus::Registered);

            tasks.push((
                chain_id,
                blockhash,
                request.block_number,
                request.proof_type,
                request.prover,
            ));
        }

        std::thread::sleep(Duration::from_millis(1));

        {
            let task_0_desc: &ProofTaskDescriptor = &(
                tasks[0].0,
                tasks[0].2,
                tasks[0].1,
                tasks[0].3,
                tasks[0].4.to_string(),
            )
                .into();
            let task_status = tama.get_task_proving_status(task_0_desc).await.unwrap().0;
            println!("{task_status:?}");
            tama.update_task_progress(
                task_0_desc.clone(),
                TaskStatus::Cancelled_NeverStarted,
                None,
            )
            .await
            .unwrap();

            let task_status = tama.get_task_proving_status(task_0_desc).await.unwrap().0;
            println!("{task_status:?}");
            assert_eq!(task_status.len(), 2);
            assert_eq!(task_status[1].0, TaskStatus::Cancelled_NeverStarted);
            assert_eq!(task_status[0].0, TaskStatus::Registered);
        }
        // -----------------------
        {
            let task_1_desc: &ProofTaskDescriptor = &(
                tasks[1].0,
                tasks[1].2,
                tasks[1].1,
                tasks[1].3,
                tasks[1].4.to_string(),
            )
                .into();
            tama.update_task_progress(task_1_desc.clone(), TaskStatus::WorkInProgress, None)
                .await
                .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_1_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(
                task_1_desc.clone(),
                TaskStatus::CancellationInProgress,
                None,
            )
            .await
            .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_1_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[2].0, TaskStatus::CancellationInProgress);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(task_1_desc.clone(), TaskStatus::Cancelled, None)
                .await
                .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_1_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 4);
                assert_eq!(task_status[3].0, TaskStatus::Cancelled);
                assert_eq!(task_status[2].0, TaskStatus::CancellationInProgress);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }
        }

        // -----------------------
        {
            let task_2_desc: &ProofTaskDescriptor = &(
                tasks[2].0,
                tasks[2].2,
                tasks[2].1,
                tasks[2].3,
                tasks[2].4.to_string(),
            )
                .into();
            tama.update_task_progress(task_2_desc.clone(), TaskStatus::WorkInProgress, None)
                .await
                .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_2_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            let proof: Vec<_> = (&mut rng).gen_iter::<u8>().take(128).collect();
            tama.update_task_progress(task_2_desc.clone(), TaskStatus::Success, Some(&proof))
                .await
                .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_2_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[2].0, TaskStatus::Success);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            assert_eq!(proof, tama.get_task_proof(task_2_desc,).await.unwrap());
        }

        // -----------------------
        {
            let task_3_desc: &ProofTaskDescriptor = &(
                tasks[3].0,
                tasks[3].2,
                tasks[3].1,
                tasks[3].3,
                tasks[3].4.to_string(),
            )
                .into();
            tama.update_task_progress(task_3_desc.clone(), TaskStatus::WorkInProgress, None)
                .await
                .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_3_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 2);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(task_3_desc.clone(), TaskStatus::NetworkFailure, None)
                .await
                .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_3_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 3);
                assert_eq!(task_status[2].0, TaskStatus::NetworkFailure);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            tama.update_task_progress(task_3_desc.clone(), TaskStatus::WorkInProgress, None)
                .await
                .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_3_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 4);
                assert_eq!(task_status[3].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[2].0, TaskStatus::NetworkFailure);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            std::thread::sleep(Duration::from_millis(1));

            let proof: Vec<_> = (&mut rng).gen_iter::<u8>().take(128).collect();
            tama.update_task_progress(
                task_3_desc.clone(),
                TaskStatus::Success,
                Some(proof.as_slice()),
            )
            .await
            .unwrap();

            {
                let task_status = tama.get_task_proving_status(task_3_desc).await.unwrap().0;
                assert_eq!(task_status.len(), 5);
                assert_eq!(task_status[4].0, TaskStatus::Success);
                assert_eq!(task_status[3].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[2].0, TaskStatus::NetworkFailure);
                assert_eq!(task_status[1].0, TaskStatus::WorkInProgress);
                assert_eq!(task_status[0].0, TaskStatus::Registered);
            }

            assert_eq!(proof, tama.get_task_proof(task_3_desc,).await.unwrap());
        }
    }
}
