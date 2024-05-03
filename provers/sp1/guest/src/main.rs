#![no_main]
sp1_zkvm::entrypoint!(main);

use raiko_lib::protocol_instance::assemble_protocol_instance;
use raiko_lib::protocol_instance::EvidenceType;
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, WrappedHeader},
};

pub fn main() {
    let input = sp1_zkvm::io::read::<GuestInput>();
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

    sp1_zkvm::io::commit(&output);

    #[cfg(test)]
    harness::zk_suits!(test_example);
}

#[test]
pub fn test_example() {
    use harness::*;
    let mut a = 1;
    let mut b = 1;
    for _ in 0..10 {
        let c = a + b;
        a = b;
        b = c;
    }
    harness::assert_eq!(b, 144);
}