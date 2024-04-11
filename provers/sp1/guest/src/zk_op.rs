use revm_precompile::{bn128::ADD_INPUT_LEN, utilities::right_pad, zk_op::ZkvmOperator, Error};
use revm_primitives::keccak256;
use sp1_zkvm::precompiles::{bn254::Bn254, secp256k1::ecrecover, utils::AffinePoint};

#[derive(Debug)]
pub struct Sp1Operator;

// TODO(Cecilia): test this shit
impl ZkvmOperator for Sp1Operator {
    fn bn128_run_add(&self, input: &[u8]) -> Result<[u8; 64], Error> {
        let input = right_pad::<ADD_INPUT_LEN>(input);

        let mut p = be_bytes_to_point(&input[..64]);
        let q = be_bytes_to_point(&input[64..]);

        p.add_assign(&q);

        Ok(point_to_be_bytes(p))
    }

    fn bn128_run_mul(&self, input: &[u8]) -> Result<[u8; 64], Error> {
        let input = right_pad::<96>(input);
        let _output = [0u8; 64];

        let mut p = be_bytes_to_point(&input[..64]);

        let k: [u32; 8] = input[64..]
            .to_owned()
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
            .collect::<Vec<u32>>()
            .try_into()
            .unwrap();

        p.mul_assign(&k);
        Ok(point_to_be_bytes(p))
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
        let mut compressed_key = [0u8; 65];
        compressed_key[0] = recid;
        compressed_key[1..].copy_from_slice(sig);
        let out =
            ecrecover(&compressed_key, msg).map_err(|e| Error::ZkvmOperatrion(e.to_string()))?;

        let mut hash = keccak256(&out[1..]);
        // truncate to 20 bytes
        hash[..12].fill(0);

        Ok(*hash)
    }
}

#[inline]
fn be_bytes_to_point(input: &[u8]) -> AffinePoint<Bn254> {
    let points = input
        .chunks_exact(32)
        .map(|chunk| {
            let mut le_chunk: [u8; 32] = chunk.try_into().expect("Input size unmatch");
            le_chunk.reverse();
            le_chunk
        })
        .collect::<Vec<_>>();

    // Init AffinePoint for sp1
    AffinePoint::<Bn254>::from(points[0], points[1])
}

#[inline]
fn point_to_be_bytes(p: AffinePoint<Bn254>) -> [u8; 64] {
    let mut p_bytes = [0u8; 64];
    let p_bytes_le = p.to_le_bytes();

    // Reverse to x, y seperatly to big-endian bytes
    p_bytes[..32].copy_from_slice(&p_bytes_le[..32].iter().rev().copied().collect::<Vec<_>>());
    p_bytes[32..].copy_from_slice(&p_bytes_le[32..].iter().rev().copied().collect::<Vec<_>>());
    p_bytes
}

/// To build test elf:
/// ~/raiko/provers/sp1/prover$ cargo build --features enable
/// The build script builds the main binary and all test binaries avaliable.
/// Todo(Cecilia): PR to `cargo prove test`
#[cfg(test)]
mod test {
    use revm_precompile::bn128;
    use revm_primitives::hex;
    use sp1_zkvm::precompiles::{bn254::Bn254, utils::AffinePoint};
    use substrate_bn::Group;

    use crate::{SimpleAlloc, HEAP};

    #[test]
    fn hex_to_point() {
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

        // Deserialize AffinePoint in Sp1
        let p_bytes = input
            .chunks_exact(32)
            .map(|chunk| {
                let mut le_chunk: [u8; 32] = chunk.try_into().expect("Input size unmatch");
                le_chunk.reverse();
                le_chunk
            })
            .collect::<Vec<_>>();
        let p = AffinePoint::<Bn254>::from(p_bytes[0], p_bytes[1]);

        let mut p_x_le = p.to_le_bytes()[..32].to_owned();
        let mut p_y_le = p.to_le_bytes()[32..].to_owned();
        p_x_le.reverse();
        p_y_le.reverse();

        assert!(p_x == *p_x_le);
        assert!(p_y == *p_y_le);
    }

    #[test]
    fn point_to_hex() {
        let G1_LE: [u8; 64] = [
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0,
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

        let p = AffinePoint::<Bn254>::from(p_x, p_y);
        let p_bytes_le = p.to_le_bytes();

        // Reverse to x, y seperatly to big-endian bytes
        let mut p_bytes_be = [0; 64];
        p_bytes_be[..32]
            .copy_from_slice(&p_bytes_le[..32].iter().rev().copied().collect::<Vec<_>>());
        p_bytes_be[32..]
            .copy_from_slice(&p_bytes_le[32..].iter().rev().copied().collect::<Vec<_>>());

        assert!(G1_LE == p_bytes_le);
        assert!(G1_BE == p_bytes_be);
    }
}
