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

    use super::risc0_k256::{
        ecdsa::{signature::DigestVerifier, RecoveryId, Signature, SigningKey, VerifyingKey},
        EncodedPoint,
    };
    use super::risc0_sha2::{Digest, Sha256};
    use hex_literal::hex;
    use sha3::Keccak256;

    /// Signature recovery test vectors
    struct RecoveryTestVector {
        pk: [u8; 33],
        msg: &'static [u8],
        sig: [u8; 64],
        recid: RecoveryId,
    }

    const RECOVERY_TEST_VECTORS: &[RecoveryTestVector] = &[
        // Recovery ID 0
        RecoveryTestVector {
            pk: hex!("021a7a569e91dbf60581509c7fc946d1003b60c7dee85299538db6353538d59574"),
            msg: b"example message",
            sig: hex!(
                "ce53abb3721bafc561408ce8ff99c909f7f0b18a2f788649d6470162ab1aa032
                 3971edc523a6d6453f3fb6128d318d9db1a5ff3386feb1047d9816e780039d52"
            ),
            recid: RecoveryId::new(false, false),
        },
        // Recovery ID 1
        RecoveryTestVector {
            pk: hex!("036d6caac248af96f6afa7f904f550253a0f3ef3f5aa2fe6838a95b216691468e2"),
            msg: b"example message",
            sig: hex!(
                "46c05b6368a44b8810d79859441d819b8e7cdc8bfd371e35c53196f4bcacdb51
                 35c7facce2a97b95eacba8a586d87b7958aaf8368ab29cee481f76e871dbd9cb"
            ),
            recid: RecoveryId::new(true, false),
        },
    ];

    #[test]
    fn public_key_recovery() {
        for vector in RECOVERY_TEST_VECTORS {
            let digest = Sha256::new_with_prefix(vector.msg);
            let sig = Signature::try_from(vector.sig.as_slice()).unwrap();
            let recid = vector.recid;
            let pk = VerifyingKey::recover_from_digest(digest, &sig, recid).unwrap();
            assert_eq!(&vector.pk[..], EncodedPoint::from(&pk).as_bytes());
        }
    }

        /// End-to-end example which ensures RFC6979 is implemented in the same
    /// way as other Ethereum libraries, using HMAC-DRBG-SHA-256 for RFC6979,
    /// and Keccak256 for hashing the message.
    ///
    /// Test vectors adapted from:
    /// <https://github.com/gakonst/ethers-rs/blob/ba00f549/ethers-signers/src/wallet/private_key.rs#L197>
    #[test]
    fn ethereum_end_to_end_example() {
        let signing_key = SigningKey::from_bytes(
            &hex!("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").into(),
        )
        .unwrap();

        let msg = hex!(
            "e9808504e3b29200831e848094f0109fc8df283027b6285cc889f5aa624eac1f55843b9aca0080018080"
        );
        let digest = Keccak256::new_with_prefix(msg);

        let (sig, recid) = signing_key.sign_digest_recoverable(digest.clone()).unwrap();
        assert_eq!(
            sig.to_bytes().as_slice(),
            &hex!("c9cf86333bcb065d140032ecaab5d9281bde80f21b9687b3e94161de42d51895727a108a0b8d101465414033c3f705a9c7b826e596766046ee1183dbc8aeaa68")
        );
        assert_eq!(recid, RecoveryId::from_byte(0).unwrap());

        let verifying_key =
            VerifyingKey::recover_from_digest(digest.clone(), &sig, recid).unwrap();

        assert_eq!(signing_key.verifying_key(), &verifying_key);
        assert!(verifying_key.verify_digest(digest, &sig).is_ok());
    }
}

