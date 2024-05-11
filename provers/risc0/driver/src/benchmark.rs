#![cfg(feature = "enable")]
#![feature(test)]
use std::env;
extern crate test;
use test::bench::run_once;
use test::Bencher;

use risc0_driver::methods::{
    ecdsa::{ECDSA_ELF, ECDSA_ID},
    sha256::{SHA256_ELF, SHA256_ID},
};
use risc0_zkvm::{default_prover, ExecutorEnv, Prover};

fn prove(elf: &[u8], id: [u32; 8]) {
    env::set_var("RISC0_PROVER", "local");
    let env = ExecutorEnv::builder().build().unwrap();
    let prover = default_prover();
    let receipt = prover.prove(env, elf).unwrap();
    receipt.verify(id).unwrap();
}

#[bench]
fn bench_sha256(b: &mut Bencher) {
    run_once(|b| {
        prove(SHA256_ELF, SHA256_ID);
        Ok(())
    });
}

#[bench]
fn bench_ecdsa(b: &mut Bencher) {
    run_once(|b| {
        prove(ECDSA_ELF, ECDSA_ID);
        Ok(())
    });
}
