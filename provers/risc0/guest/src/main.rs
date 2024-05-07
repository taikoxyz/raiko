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

use std::{
    alloc::{alloc, handle_alloc_error, Layout},
    ffi::c_void,
};

#[no_mangle]
// TODO ideally this is c_size_t, but not stabilized (not guaranteed to be usize on all archs)
unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let layout = Layout::from_size_align(size, 4).expect("unable to allocate more memory");
    let ptr = alloc(layout);

    if ptr.is_null() {
        handle_alloc_error(layout);
    }

    ptr as *mut c_void
}

#[no_mangle]
// TODO shouldn't need to zero allocated bytes since the zkvm memory is zeroed, might want to zero anyway
unsafe extern "C" fn calloc(size: usize) -> *mut c_void {
    malloc(size)
}

#[no_mangle]
unsafe extern "C" fn free(_size: *const c_void) {
    // Intentionally a no-op, since the zkvm allocator is a bump allocator
}

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
