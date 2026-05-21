#[cfg(target_os = "zkvm")]
use core::arch::asm;

/// Executes the mprotect syscall.
///
/// ### Safety
///
/// The caller must ensure that `addr` is page aligned and the bitmap consist of valid flags.
#[allow(unused_variables)]
#[no_mangle]
pub extern "C" fn syscall_mprotect(addr: *const u8, prot: u8) {
    #[cfg(target_os = "zkvm")]
    unsafe {
        asm!(
            "ecall",
            in("t0") crate::syscalls::MPROTECT,
            in("a0") addr,
            in("a1") prot,
        );
    }
}
