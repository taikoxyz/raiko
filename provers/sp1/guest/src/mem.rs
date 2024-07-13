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

#[no_mangle]
pub extern "C" fn __ctzsi2(x: u32) -> u32 {
    x.trailing_zeros()
}
