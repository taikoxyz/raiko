use k256 as sp1_k256;
use raiko_lib::primitives::keccak256;
use num_bigint::BigUint;
use revm_precompile::{bn128::ADD_INPUT_LEN, utilities::right_pad, zk_op::ZkvmOperator, Error};
use sha2 as sp1_sha2;
use sp1_curves::{weierstrass::bn254::Bn254, AffinePoint};

#[derive(Debug)]
pub struct Sp1Operator;

impl ZkvmOperator for Sp1Operator {
    fn bn128_run_add(&self, input: &[u8]) -> Result<[u8; 64], Error> {
        let input = right_pad::<ADD_INPUT_LEN>(input);
        let mut p = be_bytes_to_point(&input[..64]);
        let q = be_bytes_to_point(&input[64..]);
        p = p + q;
        Ok(point_to_be_bytes(p))
    }

    fn bn128_run_mul(&self, input: &[u8]) -> Result<[u8; 64], Error> {
        let input = right_pad::<96>(input);

        let mut p = be_bytes_to_point(&input[..64]);
        let k = BigUint::from_bytes_le(&input[64..]);
        p = p.sw_scalar_mul(&k);
        Ok(point_to_be_bytes(p))
    }

    fn bn128_run_pairing(&self, _input: &[u8]) -> Result<bool, Error> {
        unreachable!()
    }

    fn blake2_run(&self, _input: &[u8]) -> Result<[u8; 64], Error> {
        unreachable!()
    }

    fn sha256_run(&self, input: &[u8]) -> Result<[u8; 32], Error> {
        use sp1_sha2::Digest;
        Ok(sp1_sha2::Sha256::digest(input).into())
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
        use sp1_k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

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

#[inline]
fn be_bytes_to_point(input: &[u8]) -> AffinePoint<Bn254> {
    let x = BigUint::from_bytes_be(&input[..32]);
    let y = BigUint::from_bytes_be(&input[32..64]);
    // Init AffinePoint for sp1
    AffinePoint::<Bn254>::new(x, y)
}

#[inline]
fn point_to_be_bytes(p: AffinePoint<Bn254>) -> [u8; 64] {
    let mut x = [0u8; 32];
    let mut y = [0u8; 32];

    x.copy_from_slice(p.x.to_bytes_be().as_slice());
    y.copy_from_slice(p.y.to_bytes_be().as_slice());

    ([x, y]).concat().try_into().unwrap()
}

harness::zk_suits!(
    pub mod tests {
        use super::*;
        use crate::be_bytes_to_point;
        use raiko_lib::primitives::hex;
        use revm_precompile::bn128;
        use substrate_bn::Group;

        #[test]
        pub fn hex_to_point() {
            let input = hex::decode(
                "\
                18b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9\
                063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f37266\
                07c2b7f58a84bd6145f00c9c2bc0bb1a187f20ff2c92963a88019e7c6a014eed\
                06614e20c147e940f2d70da3f74c9a17df361706a4485c742bd6788478fa17d7",
            )
            .unwrap();

            // Deserialize BN point used in revm
            let p = bn128::read_point(&input).unwrap();
            // Extract x, y big-endian bytes
            let mut p_x = [0u8; 32];
            let mut p_y = [0u8; 32];
            p.x().to_big_endian(&mut p_x).unwrap();
            p.y().to_big_endian(&mut p_y).unwrap();

            println!("{p_x:?}, {p_y:?}:?");

            // Deserialize AffinePoint in Sp1
            let p = be_bytes_to_point(&input);

            assert!(p_x == *p.x.to_bytes_be());
            assert!(p_y == *p.y.to_bytes_be());
        }

        #[test]
        pub fn point_to_hex() {
            let G1_LE: [u8; 64] = [
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0,
            ];

            // Generate G1 in revm
            let p = substrate_bn::G1::one();
            // Extract x, y big-endian bytes
            let mut p_x = [0u8; 32];
            let mut p_y = [0u8; 32];
            p.x().to_big_endian(&mut p_x).unwrap();
            p.y().to_big_endian(&mut p_y).unwrap();

            // G1 bytes in big-endian
            let G1_BE = [p_x, p_y].concat();

            p_x.reverse();
            p_y.reverse();

            let p = be_bytes_to_point(&G1_BE);
            [p.x.to_bytes_le(), p.y.to_bytes_le()].concat();

            assert!(G1_LE == [p.x.to_bytes_le(), p.y.to_bytes_le()].concat());
        }
    }
);
