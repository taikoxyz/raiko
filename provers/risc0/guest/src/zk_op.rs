use k256 as risc0_k256;
use revm_precompile::{zk_op::ZkvmOperator, Error};
use sha2 as risc0_sha2;

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
        use revm_primitives::{alloy_primitives::B512, keccak256, B256};
        use risc0_k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

        // parse signature
        let mut sig = Signature::from_slice(sig.as_slice()).map_err(|_| {
            Error::ZkvmOperatrion("Patched k256 deserialize signature failed".to_string())
        })?;

        // normalize signature and flip recovery id if needed.
        if let Some(sig_normalized) = sig.normalize_s() {
            sig = sig_normalized;
            recid ^= 1;
        }
        let recid = RecoveryId::from_byte(recid).expect("recovery ID is valid");

        // recover key
        let recovered_key = VerifyingKey::recover_from_prehash(&msg[..], &sig, recid)
            .map_err(|_| Error::ZkvmOperatrion("Patched k256 recover key failed".to_string()))?;
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

#[cfg(test)]
mod test {

    #[test]
    fn fib() {
        let mut a = 1;
        let mut b = 1;
        for _ in 0..10 {
            let c = a + b;
            a = b;
            b = c;
        }
        assert_eq!(b, 144);
    }
}
