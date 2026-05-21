#[cfg(target_os = "zkvm")]
use core::arch::asm;

/// Uint256 addition operation with carry.
///
/// Computes (a + b + c) and returns:
/// - d: the low 256 bits (result % 2^256)
/// - e: the high 256 bits (result // 2^256)
///
/// ### Safety
///
/// The caller must ensure that `a`, `b`, `c`, `d`, and `e` are valid pointers to data that is
/// aligned along an eight byte boundary.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_uint256_add_with_carry(
    a: *const [u64; 4],
    b: *const [u64; 4],
    c: *const [u64; 4],
    d: *mut [u64; 4],
    e: *mut [u64; 4],
) {
    #[cfg(target_os = "zkvm")]
    unsafe {
        asm!(
            "ecall",
            in("t0") crate::syscalls::UINT256_ADD_CARRY,
            in("a0") a,
            in("a1") b,
            in("a2") c,
            in("a3") d,
            in("a4") e,
        );
    }

    #[cfg(not(target_os = "zkvm"))]
    unreachable!()
}

/// Uint256 multiply-add operation with carry.
///
/// Computes (a * b + c) and returns:
/// - d: the low 256 bits (result % 2^256)
/// - e: the high 256 bits (result // 2^256)
///
/// ### Safety
///
/// The caller must ensure that `a`, `b`, `c`, `d`, and `e` are valid pointers to data that is
/// aligned along an eight byte boundary.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_uint256_mul_with_carry(
    a: *const [u64; 4],
    b: *const [u64; 4],
    c: *const [u64; 4],
    d: *mut [u64; 4],
    e: *mut [u64; 4],
) {
    #[cfg(target_os = "zkvm")]
    unsafe {
        asm!(
            "ecall",
            in("t0") crate::syscalls::UINT256_MUL_CARRY,
            in("a0") a,
            in("a1") b,
            in("a2") c,
            in("a3") d,
            in("a4") e,
        );
    }

    #[cfg(not(target_os = "zkvm"))]
    unreachable!()
}
