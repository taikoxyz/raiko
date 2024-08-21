use k256 as risc0_k256;
use raiko_lib::primitives::keccak256;
use revm_precompile::{zk_op::ZkvmOperator, Error};
use sha2 as risc0_sha2;

#[derive(Debug)]
pub struct Risc0Operator;

impl ZkvmOperator for Risc0Operator {
    fn bn128_run_add(&self, _input: &[u8]) -> Result<[u8; 64], Error> {
        unreachable!()
    }

    fn bn128_run_mul(&self, _input: &[u8]) -> Result<[u8; 64], Error> {
        unreachable!()
    }

    fn bn128_run_pairing(&self, _input: &[u8]) -> Result<bool, Error> {
        unreachable!()
    }

    fn blake2_run(&self, _input: &[u8]) -> Result<[u8; 64], Error> {
        unreachable!()
    }

    fn sha256_run(&self, input: &[u8]) -> Result<[u8; 32], Error> {
        use risc0_sha2::Digest;
        Ok(risc0_sha2::Sha256::digest(input).into())
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
        mut recid: u8,
        msg: &[u8; 32],
    ) -> Result<[u8; 32], Error> {
        use risc0_k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

        // parse signature
        let mut sig = Signature::from_slice(sig.as_slice()).map_err(|_| {
            Error::ZkvmOperation("Patched k256 deserialize signature failed".to_string())
        })?;
        // normalize signature and flip recovery id if needed.
        if let Some(sig_normalized) = sig.normalize_s() {
            sig = sig_normalized;
            recid ^= 1;
        }
        let recid = RecoveryId::from_byte(recid).expect("recovery ID is valid");
        // recover key
        let recovered_key = VerifyingKey::recover_from_prehash(&msg[..], &sig, recid)
            .map_err(|_| Error::ZkvmOperation("Patched k256 recover key failed".to_string()))?;
        // hash it
        let mut hash = keccak256(
            &recovered_key
                .to_encoded_point(/* compress = */ false)
                .as_bytes()[1..],
        );

        // truncate to 20 bytes
        hash[..12].fill(0);
        Ok(*hash)
    }
}

harness::zk_suits!(
    pub mod tests {
        #[test]
        pub fn test_sha256() {
            use crate::risc0_sha2::{Digest, Sha256};
            use harness::*;
            use raiko_lib::primitives::hex;

            let test_ves = [
                (
                    "",
                    hex!("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
                ),
                (
                    "The quick brown fox jumps over the lazy dog",
                    hex!("d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"),
                ),
                (
                    "hello",
                    hex!("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
                ),
            ];

            for v in test_ves.iter() {
                let (input, expected) = *v;
                let result: [u8; 32] = Sha256::digest(input.as_bytes()).into();
                // Don't change, this `assert!` custom defimed in `harness` crate.
                assert!(result == expected);
            }
        }
    }
);