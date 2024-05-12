// Raiko
// Copyright (c) 2024 Taiko Labs
// Licensed and distributed under either of
//   * MIT license (license terms in the root directory or at http://opensource.org/licenses/MIT).
//   * Apache v2 license (license terms in the root directory or at http://www.apache.org/licenses/LICENSE-2.0).
// at your option. This file may not be copied, modified, or distributed except according to those terms.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::fs;

    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;
    use tempfile::tempdir;

    use raiko_primitives::B256;
    use task_manager::{TaskDb, TaskManager, TaskManagerError, TaskProofsys, TaskStatus};

    #[test]
    fn test_enqueue_task() {
        let dir = std::env::current_dir().unwrap().join("tests");
        let file = dir.as_path().join("test_enqueue_task.sqlite");
        if file.exists() {
            fs::remove_file(&file).unwrap()
        };


        let db = TaskDb::open_or_create(&file).unwrap();
        let mut tama = TaskDb::manage(&db).unwrap();

        let mut rng = ChaCha8Rng::seed_from_u64(123);

        let chain_id = 100;
        let blockhash = B256::random();
        let proofsys = TaskProofsys::Risc0;
        let payload_length = rng.gen_range(20..200);
        let payload: Vec<u8> = rng.gen_iter::<u8>().take(payload_length).collect();
        let submitter = "test_enqueue_task";

        tama.enqueue_task(
            chain_id,
            blockhash,
            proofsys,
            &payload,
            submitter
        ).unwrap();
    }
}
