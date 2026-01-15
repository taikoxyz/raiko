use std::{
    alloc::{alloc, alloc_zeroed, handle_alloc_error, Layout},
    ffi::c_void,
};

/// This is a basic implementation of custom memory allocation functions that mimic C-style memory management.
/// This implementation is designed to be used in ZkVM where we cross-compile Rust code with C
/// due to the dependency of c-kzg.

const MALLOC_ALIGN: usize = 16;

#[no_mangle]
// TODO ideally this is c_size_t, but not stabilized (not guaranteed to be usize on all archs)
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let layout = match Layout::from_size_align(size, MALLOC_ALIGN) {
        Ok(layout) => layout,
        Err(_) => return std::ptr::null_mut(),
    };
    let ptr = alloc(layout);

    if ptr.is_null() {
        handle_alloc_error(layout);
    }

    ptr as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn calloc(nobj: usize, size: usize) -> *mut c_void {
    let total = match nobj.checked_mul(size) {
        Some(total) => total,
        None => return std::ptr::null_mut(),
    };

    let layout = match Layout::from_size_align(total, MALLOC_ALIGN) {
        Ok(layout) => layout,
        Err(_) => return std::ptr::null_mut(),
    };

    let ptr = alloc_zeroed(layout);
    if ptr.is_null() {
        handle_alloc_error(layout);
    }

    ptr as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn free(_ptr: *mut c_void) {
    // Intentionally a no-op, since the zkvm allocator is a bump allocator
}

#[no_mangle]
pub extern "C" fn __ctzsi2(x: u32) -> u32 {
    x.trailing_zeros()
}
