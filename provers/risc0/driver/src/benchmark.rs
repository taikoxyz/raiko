use criterion::{black_box, criterion_group, criterion_main, Criterion};
use risc0_zkvm::{default_prover, ExecutorEnv};
use methods::{
    risc0_guest::{RISC0_GUEST_ELF, RISC0_GUEST_ID},
    test_risc0_guest::{TEST_RISC0_GUEST_ELF, TEST_RISC0_GUEST_ID},
};

fn sha256_benchmark(c: &mut Criterion) {
    c.bench_function("sha256", |b| {
        b.iter(|| {
            let data = black_box("The quick brown fox jumps over the lazy dog");
            Sha256::digest(data.as_bytes());

           
            let env = ExecutorEnv::builder().build().unwrap();
            let prover = default_prover();
            let receipt = prover.prove(env, TEST_RISC0_GUEST_ELF).unwrap();
            receipt.verify(TEST_RISC0_GUEST_ID).unwrap();
    
        });
    });
}

fn sha256_benchmark(c: &mut Criterion) {
    c.bench_function("ecdsa", |b| {
        b.iter(|| {
            let data = black_box("The quick brown fox jumps over the lazy dog");
            Sha256::digest(data.as_bytes());

           
            let env = ExecutorEnv::builder().build().unwrap();
            let prover = default_prover();
            let receipt = prover.prove(env, TEST_RISC0_GUEST_ELF).unwrap();
            receipt.verify(TEST_RISC0_GUEST_ID).unwrap();
    
        });
    });
}

criterion_group!(benches, sha256_benchmark);
criterion_main!(benches);
