use std::{
    alloc::{alloc, handle_alloc_error, Layout},
    ffi::c_void,
};

/// This is a basic implementation of custom memory allocation functions that mimic C-style memory management.
/// This implementation is designed to be used in ZkVM where we cross-compile Rust code with C
/// due to the dependency of c-kzg. This modification also requires env var:
///     $ CC="gcc"
///     $ CC_riscv32im-risc0-zkvm-elf="/opt/riscv/bin/riscv32-unknown-elf-gcc"
/// which is set in the build pipeline

#[no_mangle]
// TODO ideally this is c_size_t, but not stabilized (not guaranteed to be usize on all archs)
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let layout = Layout::from_size_align(size, 4).expect("unable to allocate more memory");
    let ptr = alloc(layout);

    if ptr.is_null() {
        handle_alloc_error(layout);
    }

    ptr as *mut c_void
}

#[no_mangle]
// TODO shouldn't need to zero allocated bytes since the zkvm memory is zeroed, might want to zero anyway
pub unsafe extern "C" fn calloc(nobj: usize, size: usize) -> *mut c_void {
    malloc(nobj * size)
}

#[no_mangle]
pub unsafe extern "C" fn free(_size: *const c_void) {
    // Intentionally a no-op, since the zkvm allocator is a bump allocator
}

/// LLVM intrinsics that is not available in in the RiscV asm set used in SP1
/// needed by rust_secp256k1
#[no_mangle]
pub extern "C" fn __ctzsi2(x: u32) -> u32 {
    x.trailing_zeros()
}


use rand::{rngs::StdRng, Rng, SeedableRng};
use std::sync::Mutex;
/// https://github.com/succinctlabs/sp1/blob/d5a1423ffe4740b154b60764f17b201c7d94e80e/zkvm/entrypoint/src/syscalls/sys.rs#L8
/// The random number generator seed for the zkVM.
/// Needed for tag = v1.0.5-testnet, in latest main branch we don't need this
const PRNG_SEED: u64 = 0x123456789abcdef0;

lazy_static::lazy_static! {
    /// A lazy static to generate a global random number generator.
    static ref RNG: Mutex<StdRng> = Mutex::new(StdRng::seed_from_u64(PRNG_SEED));
}
/// A lazy static to print a warning once for using the `sys_rand` system call.
static SYS_RAND_WARNING: std::sync::Once = std::sync::Once::new();

/// Generates random bytes.
///
/// # Safety
///
/// Make sure that `buf` has at least `nwords` words.
#[no_mangle]
pub unsafe extern "C" fn sys_rand(recv_buf: *mut u8, words: usize) {
    SYS_RAND_WARNING.call_once(|| {
        println!("WARNING: Using insecure random number generator.");
    });
    let mut rng = RNG.lock().unwrap();
    for i in 0..words {
        let element = recv_buf.add(i);
        *element = rng.gen();
    }
}