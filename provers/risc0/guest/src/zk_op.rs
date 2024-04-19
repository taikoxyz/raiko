use revm_precompile::{bn128::ADD_INPUT_LEN, utilities::right_pad, zk_op::ZkvmOperator, Error};

#[derive(Debug)]
pub struct Risc0Operator;

impl ZkvmOperator for Risc0Operator {
    fn bn128_run_add(&self, input: &[u8]) -> Result<[u8; 64], Error> {
        unreachable!()
    }

    fn bn128_run_mul(&self, input: &[u8]) -> Result<[u8; 64], Error> {
        unreachable!()
    }

    fn bn128_run_pairing(&self, _input: &[u8]) -> Result<bool, Error> {
        unreachable!()
    }

    fn blake2_run(&self, _input: &[u8]) -> Result<[u8; 64], Error> {
        unreachable!()
    }

    fn sha256_run(&self, _input: &[u8]) -> Result<[u8; 32], Error> {
        // Handle through [patch.crates-io]
        // sha2-v0-10-8 = {
        //     git = "https://github.com/sp1-patches/RustCrypto-hashes",
        //     package = "sha2",
        //     branch = "v0.10.8"
        // }
        unreachable!()
    }

    fn ripemd160_run(&self, _input: &[u8]) -> Result<[u8; 32], Error> {
        unreachable!()
    }

    fn modexp_run(&self, _base: &[u8], _exp: &[u8], _modulus: &[u8]) -> Result<Vec<u8>, Error> {
        unreachable!()
    }

    fn secp256k1_ecrecover(
        &self,
        sig: &[u8; 64],
        recid: u8,
        msg: &[u8; 32],
    ) -> Result<[u8; 32], Error> {
        unreachable!()
    }
}
