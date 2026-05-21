#[cfg(target_os = "zkvm")]
use core::arch::asm;

/// Uint256 multiplication operation.
///
/// The result is written over the first input.
///
/// ### Safety
///
/// The caller must ensure that `x` and `y` are valid pointers to data that is aligned along an
/// eight byte boundary.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_uint256_mulmod(x: *mut [u64; 4], y: *const [u64; 4]) {
    #[cfg(target_os = "zkvm")]
    unsafe {
        asm!(
            "ecall",
            in("t0") crate::syscalls::UINT256_MUL,
            in("a0") x,
            in("a1") y,
        );
    }

    #[cfg(not(target_os = "zkvm"))]
    unreachable!()
}
