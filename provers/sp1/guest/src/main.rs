#![no_main]
sp1_zkvm::entrypoint!(main);

use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, WrappedHeader},
    protocol_instance::{assemble_protocol_instance, EvidenceType},
};

pub fn main() {
    let input = sp1_zkvm::io::read::<GuestInput>();

    // revm_precompile::zk_op::ZKVM_OPERATOR.get_or_init(|| Box::new(Sp1Operator {}));
    // revm_precompile::zk_op::ZKVM_OPERATIONS
    //     .set(Box::new(vec![]))
    //     .expect("Failed to set ZkvmOperations");

    let build_result = TaikoStrategy::build_from(&input);

    let output = match &build_result {
        Ok((header, mpt_node)) => {
            let pi = assemble_protocol_instance(&input, &header)
                .expect("Failed to assemble protocol instance")
                .instance_hash(EvidenceType::Succinct);
            GuestOutput::Success((
                WrappedHeader {
                    header: header.clone(),
                },
                pi,
            ))
        }
        Err(_) => GuestOutput::Failure,
    };

    sp1_zkvm::io::write(&output);
}

// use revm_precompile::{
//     bn128::{read_point, ADD_INPUT_LEN},
//     utilities::right_pad,
//     zk_op::{Operation, ZkvmOperator},
//     Error,
// };
use sp1_zkvm::precompiles::{bn254::Bn254, syscall_bn254_add, utils::AffinePoint};

#[derive(Debug)]
pub struct Sp1Operator;

// impl ZkvmOperator for Sp1Operator {
//     fn bn128_run_add(&self, input: &[u8]) -> Result<[u8; 64], Error> {
//         let input = right_pad::<ADD_INPUT_LEN>(input);
//         let mut output = [0u8; 64];

//         let p = read_point(&input[..64])?;
//         let q = read_point(&input[64..])?;

//         // Extract x, y big-endian bytes
//         let mut p_x = [0u8; 32];
//         let mut p_y = [0u8; 32];
//         p.x()
//             .to_big_endian(&mut p_x)
//             .map_err(|e| Error::ZkvmOperatrion("Failed to extract input BE bytes".to_string()))?;
//         p.y()
//             .to_big_endian(&mut p_y)
//             .map_err(|e| Error::ZkvmOperatrion("Failed to extract input BE bytes".to_string()))?;

//         // Extract x, y big-endian bytes
//         let mut q_x = [0u8; 32];
//         let mut q_y = [0u8; 32];
//         q.x()
//             .to_big_endian(&mut q_x)
//             .map_err(|e| Error::ZkvmOperatrion("Failed to extract input BE bytes".to_string()))?;
//         q.y()
//             .to_big_endian(&mut q_y)
//             .map_err(|e| Error::ZkvmOperatrion("Failed to extract input BE bytes".to_string()))?;

//         // Reverse all big-endian to little-endian bytes
//         [p_x, p_y, q_x, q_y]
//             .iter_mut()
//             .for_each(|bytes| bytes.reverse());

//         // Init AffinePoint for sp1
//         let mut p = AffinePoint::<Bn254>::from(p_x, p_y);
//         let q = AffinePoint::<Bn254>::from(q_x, q_y);
//         p.add_assign(&q);

//         // Convert resultant AffinePoint to x, y bytes concat in little-endian
//         let p_bytes = p.to_le_bytes();

//         // Reverse to x, y seperatly to big-endian bytes
//         output[..32].copy_from_slice(&p_bytes[..32].iter().rev().copied().collect::<Vec<_>>());
//         output[32..].copy_from_slice(&p_bytes[32..].iter().rev().copied().collect::<Vec<_>>());
//         Ok(output)
//     }

//     fn bn128_run_mul(&self, input: &[u8]) -> Result<[u8; 64], Error> {
//         todo!()
//     }

//     fn bn128_run_pairing(&self, input: &[u8]) -> Result<bool, Error> {
//         todo!()
//     }

//     fn blake2_run(&self, input: &[u8]) -> Result<[u8; 64], Error> {
//         todo!()
//     }

//     fn sha256_run(&self, input: &[u8]) -> Result<[u8; 32], Error> {
//         todo!()
//     }

//     fn ripemd160_run(&self, input: &[u8]) -> Result<[u8; 32], Error> {
//         todo!()
//     }

//     fn modexp_run(&self, base: &[u8], exp: &[u8], modulus: &[u8]) -> Result<Vec<u8>, Error> {
//         todo!()
//     }

//     fn secp256k1_ecrecover(
//         &self,
//         sig: &[u8; 64],
//         recid: u8,
//         msg: &[u8; 32],
//     ) -> Result<[u8; 32], Error> {
//         todo!()
//     }
// }

#[inline]
pub fn bn254_add(p: &mut [u8; 64], q: &[u8; 64]) {
    unsafe { syscall_bn254_add(p.as_mut_ptr() as *mut u32, q.as_ptr() as *const u32) }
}

// Convert a u8 array to a u32 array
pub fn u8_to_u32(arr: &[u8; 32]) -> [u32; 8] {
    let mut res = [0u32; 8];
    for i in 0..8 {
        res[i] = u32::from_le_bytes(arr[i * 4..(i + 1) * 4].try_into().unwrap());
    }
    res
}
