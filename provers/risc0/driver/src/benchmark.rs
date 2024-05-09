use criterion::{black_box, criterion_group, criterion_main, Criterion};
use risc0_zkvm::{default_prover, ExecutorEnv};
use methods::{
    SHA256_ELF::{SHA256_ELF, SHA256_ID},
    ecdsa::{ECDSA_ELF, ECDSA_ID},
};

fn sha256_benchmark(c: &mut Criterion) {
    c.bench_function("sha256", |b| {
        b.iter(|| {
            let data = black_box("The quick brown fox jumps over the lazy dog");
            Sha256::digest(data.as_bytes());

           
            let env = ExecutorEnv::builder().build().unwrap();
            let prover = default_prover();
            let receipt = prover.prove(env, SHA256_ELF).unwrap();
            receipt.verify(SHA256_ID).unwrap();
    
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
            let receipt = prover.prove(env, ECDSA_ELF).unwrap();
            receipt.verify(ECDSA_ID).unwrap();
    
        });
    });
}

criterion_group!(benches, sha256_benchmark);
criterion_main!(benches);
