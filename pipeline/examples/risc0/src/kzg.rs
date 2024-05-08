#![no_main]
#![allow(unused_imports)]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(run);
use harness::*;

use std::{
    alloc::{alloc, handle_alloc_error, Layout},
    ffi::c_void,
};
use c_kzg_taiko::{Blob, KzgCommitment, KzgSettings};
use alloy_primitives::B256;
use sha2::{Digest as _, Sha256};

pub const BYTES_PER_BLOB: usize = 131072;
const KZG_TRUST_SETUP_DATA: &[u8] = include_bytes!("../../../../kzg_settings_raw.bin");
pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;


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


pub fn kzg_to_versioned_hash(commitment: KzgCommitment) -> B256 {
    let mut res = sha2::Sha256::digest(commitment.as_slice());
    res[0] = VERSIONED_HASH_VERSION_KZG;
    B256::new(res.into())
}

#[test]
fn test_kzg() {
    let dummy_input = [0; BYTES_PER_BLOB];
    println!("kzg check enabled!");
    let mut data = Vec::from(KZG_TRUST_SETUP_DATA);
    let kzg_settings = KzgSettings::from_u8_slice(&mut data);
    let blob = Blob::from_bytes(&dummy_input).unwrap();
    let kzg_commit = KzgCommitment::blob_to_kzg_commitment(
        &blob,
        &kzg_settings,
    )
    .unwrap();
    let versioned_hash = kzg_to_versioned_hash(kzg_commit);
}


fn run() {

    println!("calloc, malloc");
    unsafe {
        let c = calloc(100);
        let m = malloc(200);
        free(c);
        free(m);
    }

    #[cfg(test)]
    harness::zk_suits!(test_kzg);
}