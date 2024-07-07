#![cfg(feature = "enable")]
#![feature(test)]
extern crate test;
use sp1_sdk::{ProverClient, SP1Stdin};
use test::{bench::run_once, Bencher};

const BN254_ADD_ELF: &[u8] = include_bytes!("../../guest/elf/bn254-add");
const BN254_MUL_ELF: &[u8] = include_bytes!("../../guest/elf/bn254-mul");
const ECDSA_ELF: &[u8] = include_bytes!("../../guest/elf/ecdsa");
const SHA256_ELF: &[u8] = include_bytes!("../../guest/elf/sha256");

fn prove(elf: &[u8]) {
    let client = ProverClient::new();
    let stdin = SP1Stdin::new();
    let (pk, vk) = client.setup(elf);
    let proof = client.prove(&pk, stdin).expect("Sp1: proving failed");
    client
        .verify(&proof, &vk)
        .expect("Sp1: verification failed");
}

#[bench]
fn bench_sha256(b: &mut Bencher) {
    run_once(|b| {
        prove(SHA256_ELF);
        Ok(())
    });
}

#[bench]
fn bench_ecdsa(b: &mut Bencher) {
    run_once(|b| {
        prove(ECDSA_ELF);
        Ok(())
    });
}

#[bench]
fn bench_bn254_add(b: &mut Bencher) {
    run_once(|b| {
        prove(BN254_ADD_ELF);
        Ok(())
    });
}

#[bench]
fn bench_bn254_mul(b: &mut Bencher) {
    run_once(|b| {
        prove(BN254_MUL_ELF);
        Ok(())
    });
}
