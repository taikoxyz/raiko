#[cfg(target_os = "zkvm")]
use core::arch::asm;

/// Multiplication operation between a 256-bit and a 2048-bit unsigned integer.
///
/// The low 2048-bit result is written to the `lo` pointer, and the high 256-bit overflow is written
/// to the `hi` pointer.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_u256x2048_mul(
    a: *const [u64; 4],
    b: *const [u64; 32],
    lo: *mut [u64; 32],
    hi: *mut [u64; 4],
) {
    #[cfg(target_os = "zkvm")]
    unsafe {
        asm!(
            "ecall",
            in("t0") crate::syscalls::U256XU2048_MUL,
            in("a0") a,
            in("a1") b,
            in("a2") lo,
            in("a3") hi,
        );
    }

    #[cfg(not(target_os = "zkvm"))]
    unreachable!()
}
