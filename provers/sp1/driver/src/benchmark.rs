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
    sp1_sdk::utils::setup_logger();
    let client = ProverClient::new();
    let stdin = SP1Stdin::new();
    let (pk, vk) = client.setup(elf);
    let proof = client.prove(&pk, stdin).run().expect("Sp1: proving failed");
    client
        .verify(&proof, &vk)
        .expect("Sp1: verification failed");
}

#[bench]
fn bench_sha256(_: &mut Bencher) {
    run_once(|_| {
        prove(SHA256_ELF);
        Ok(())
    })
    .unwrap();
}

#[bench]
fn bench_ecdsa(_: &mut Bencher) {
    run_once(|_| {
        prove(ECDSA_ELF);
        Ok(())
    })
    .unwrap();
}

#[bench]
fn bench_bn254_add(_: &mut Bencher) {
    run_once(|_| {
        prove(BN254_ADD_ELF);
        Ok(())
    })
    .unwrap();
}

#[bench]
fn bench_bn254_mul(_: &mut Bencher) {
    run_once(|_| {
        prove(BN254_MUL_ELF);
        Ok(())
    })
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        prove(SHA256_ELF);
    }

    #[test]
    fn test_ecdsa() {
        prove(ECDSA_ELF);
    }

    #[test]
    fn test_bn254_add() {
        prove(BN254_ADD_ELF);
    }

    #[test]
    fn test_bn254_mul() {
        prove(BN254_MUL_ELF);
    }
}
