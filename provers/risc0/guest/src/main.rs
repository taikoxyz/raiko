#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);

use raiko_lib::protocol_instance::assemble_protocol_instance;
use raiko_lib::protocol_instance::EvidenceType;
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, WrappedHeader},
};
#[cfg(test)]
use harness::*;

fn main() {
    let input: GuestInput = env::read();
    let build_result = TaikoStrategy::build_from(&input);

    // TODO: cherry-pick risc0 latest output
    let output = match &build_result {
        Ok((header, _mpt_node)) => {
            let pi = assemble_protocol_instance(&input, &header)
                .expect("Failed to assemble protocol instance")
                .instance_hash(EvidenceType::Risc0);
            GuestOutput::Success((
                WrappedHeader {
                    header: header.clone(),
                },
                pi,
            ))
        }
        Err(_) => GuestOutput::Failure,
    };

    env::commit(&output);

    #[cfg(test)]
    harness::zk_suits!(test_example);
}

#[test]
fn test_example() {
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
