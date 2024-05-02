#![no_main]
sp1_zkvm::entrypoint!(main);

use raiko_lib::protocol_instance::assemble_protocol_instance;
use raiko_lib::protocol_instance::EvidenceType;
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, WrappedHeader},
};

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
}
