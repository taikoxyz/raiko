#![no_main]
#![allow(unused_imports)]
sp1_zkvm::entrypoint!(main);
use harness::*;
use std::hint::black_box;


use std::{
    alloc::{alloc, handle_alloc_error, Layout},
    ffi::c_void,
};
use c_kzg_taiko::{Blob, KzgCommitment, KzgSettings};
use alloy_primitives::B256;
use sha2::{Digest as _, Sha256};

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
pub const BYTES_PER_BLOB: usize = 131072;
const KZG_TRUST_SETUP_DATA: &[u8] = include_bytes!("../../../../kzg_settings_raw.bin");
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
    // let versioned_hash = kzg_to_versioned_hash(kzg_commit);
}
pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;
pub fn kzg_to_versioned_hash(commitment: KzgCommitment) -> B256 {
    let mut res = sha2::Sha256::digest(commitment.as_slice());
    res[0] = VERSIONED_HASH_VERSION_KZG;
    B256::new(res.into())
}

fn fibonacci(n: u32) -> u32 {
    let mut nums = vec![1, 1];
    for _ in 0..n {
        let mut c = nums[nums.len() - 1] + nums[nums.len() - 2];
        c %= 7910;
        nums.push(c);
    }
    nums[nums.len() - 1]
}

pub fn main() {
    // let result = black_box(fibonacci(black_box(1000)));
    // println!("result: {}", result);

    // #[cfg(test)]
    // harness::zk_suits!(test_fib, test_fail);

    test_kzg();
}

#[test]
pub fn test_fib() {
    let mut a = 1;
    let mut b = 1;
    for _ in 0..10 {
        let c = a + b;
        a = b;
        b = c;
    }
    harness::assert_eq!(b, 144);
}

#[test]
pub fn test_fail() {
    harness::assert_eq!(1, 2);
}
