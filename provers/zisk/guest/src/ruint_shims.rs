//! Shims for zisk-patch-ruint extern "C" symbols.
//!
//! The patched ruint declares 7 `extern "C"` functions for 256-bit
//! arithmetic.  This module provides `#[no_mangle]` implementations
//! that delegate to ziskos syscalls where possible and fall back to
//! pure software for operations without a direct syscall (divrem, pow).

use ziskos::syscalls::{
    syscall_arith256, syscall_arith256_mod, SyscallArith256ModParams, SyscallArith256Params,
};

const ZERO: [u64; 4] = [0; 4];
const ONE: [u64; 4] = [1, 0, 0, 0];

// ==================== Modular operations ====================

/// `result = a mod m`
///
/// Implemented as `d = (a · 1 + 0) mod m`.
#[no_mangle]
pub unsafe extern "C" fn redmod256_c(a: *const u64, m: *const u64, result: *mut u64) {
    let a_arr = &*(a as *const [u64; 4]);
    let m_arr = &*(m as *const [u64; 4]);
    let r_arr = &mut *(result as *mut [u64; 4]);

    let mut params = SyscallArith256ModParams {
        a: a_arr,
        b: &ONE,
        c: &ZERO,
        module: m_arr,
        d: r_arr,
    };
    syscall_arith256_mod(&mut params);
}

/// `result = (a + b) mod m`
///
/// Implemented as `d = (1 · a + b) mod m`.
#[no_mangle]
pub unsafe extern "C" fn addmod256_c(
    a: *const u64,
    b: *const u64,
    m: *const u64,
    result: *mut u64,
) {
    let a_arr = &*(a as *const [u64; 4]);
    let b_arr = &*(b as *const [u64; 4]);
    let m_arr = &*(m as *const [u64; 4]);
    let r_arr = &mut *(result as *mut [u64; 4]);

    let mut params = SyscallArith256ModParams {
        a: a_arr,
        b: &ONE,
        c: b_arr,
        module: m_arr,
        d: r_arr,
    };
    syscall_arith256_mod(&mut params);
}

/// `result = (a * b) mod m`
///
/// Implemented as `d = (a · b + 0) mod m`.
#[no_mangle]
pub unsafe extern "C" fn mulmod256_c(
    a: *const u64,
    b: *const u64,
    m: *const u64,
    result: *mut u64,
) {
    let a_arr = &*(a as *const [u64; 4]);
    let b_arr = &*(b as *const [u64; 4]);
    let m_arr = &*(m as *const [u64; 4]);
    let r_arr = &mut *(result as *mut [u64; 4]);

    let mut params = SyscallArith256ModParams {
        a: a_arr,
        b: b_arr,
        c: &ZERO,
        module: m_arr,
        d: r_arr,
    };
    syscall_arith256_mod(&mut params);
}

// ==================== Multiplication ====================

/// Wrapping 256×256→256 multiply (low 256 bits of full product).
///
/// Uses `arith256` syscall: `a * b + 0 = dh | dl`, returns `dl`.
#[no_mangle]
pub unsafe extern "C" fn wmul256_c(a: *const u64, b: *const u64, result: *mut u64) {
    let a_arr = &*(a as *const [u64; 4]);
    let b_arr = &*(b as *const [u64; 4]);
    let r_arr = &mut *(result as *mut [u64; 4]);

    let mut dh = [0u64; 4];
    let mut params = SyscallArith256Params {
        a: a_arr,
        b: b_arr,
        c: &ZERO,
        dl: r_arr,
        dh: &mut dh,
    };
    syscall_arith256(&mut params);
}

/// Overflowing 256×256→256 multiply. Returns `true` if the result overflows.
///
/// Uses `arith256` syscall: `a * b + 0 = dh | dl`.
/// `result = dl`, overflow = `dh != 0`.
#[no_mangle]
pub unsafe extern "C" fn omul256_c(a: *const u64, b: *const u64, result: *mut u64) -> bool {
    let a_arr = &*(a as *const [u64; 4]);
    let b_arr = &*(b as *const [u64; 4]);
    let r_arr = &mut *(result as *mut [u64; 4]);

    let mut dh = [0u64; 4];
    let mut params = SyscallArith256Params {
        a: a_arr,
        b: b_arr,
        c: &ZERO,
        dl: r_arr,
        dh: &mut dh,
    };
    syscall_arith256(&mut params);

    (dh[0] | dh[1] | dh[2] | dh[3]) != 0
}

// ==================== Division ====================

/// 256-bit unsigned division with remainder: `a = q * b + r`.
///
/// No division syscall exists in ziskos, so this is implemented as:
/// 1. `r = a mod b` via `arith256_mod` syscall.
/// 2. `q` via software binary long division.
///
/// When `b == 0`, sets `q = 0, r = 0` (matches ruint's convention).
#[no_mangle]
pub unsafe extern "C" fn divrem256_c(a: *const u64, b: *const u64, q: *mut u64, r: *mut u64) {
    let a_arr = &*(a as *const [u64; 4]);
    let b_arr = &*(b as *const [u64; 4]);
    let q_arr = &mut *(q as *mut [u64; 4]);
    let r_arr = &mut *(r as *mut [u64; 4]);

    // Handle b == 0
    if (b_arr[0] | b_arr[1] | b_arr[2] | b_arr[3]) == 0 {
        *q_arr = ZERO;
        *r_arr = ZERO;
        return;
    }

    // Handle a == 0
    if (a_arr[0] | a_arr[1] | a_arr[2] | a_arr[3]) == 0 {
        *q_arr = ZERO;
        *r_arr = ZERO;
        return;
    }

    // Compute r = a mod b via syscall
    let mut params = SyscallArith256ModParams {
        a: a_arr,
        b: &ONE,
        c: &ZERO,
        module: b_arr,
        d: r_arr,
    };
    syscall_arith256_mod(&mut params);

    // Now compute q = (a - r) / b.
    // Since a = q*b + r, we have (a - r) = q*b, so (a - r) is exactly divisible by b.
    // We compute a - r first, then perform exact division via binary long division.

    // a_sub_r = a - r
    let mut a_sub_r = [0u64; 4];
    let mut borrow: u64 = 0;
    for i in 0..4 {
        let (s1, b1) = a_arr[i].overflowing_sub(r_arr[i]);
        let (s2, b2) = s1.overflowing_sub(borrow);
        a_sub_r[i] = s2;
        borrow = (b1 as u64) + (b2 as u64);
    }

    // If a_sub_r == 0, then q = 0 (a == r, i.e., a < b)
    if (a_sub_r[0] | a_sub_r[1] | a_sub_r[2] | a_sub_r[3]) == 0 {
        *q_arr = ZERO;
        return;
    }

    // Binary long division of a_sub_r / b.
    // Since a_sub_r is exactly divisible by b, r_div will be 0.
    // Standard restoring division: process bits from MSB to LSB.
    *q_arr = ZERO;
    let mut remainder = [0u64; 4];

    // Find the MSB of a_sub_r
    let msb = msb_pos(&a_sub_r);

    for i in (0..=msb).rev() {
        // remainder = remainder << 1
        shl1_256(&mut remainder);

        // remainder.bit(0) = a_sub_r.bit(i)
        let limb = i / 64;
        let bit = i % 64;
        remainder[0] |= (a_sub_r[limb] >> bit) & 1;

        // if remainder >= b: remainder -= b; q.bit(i) = 1
        if cmp_256(&remainder, b_arr) >= 0 {
            sub_256_inplace(&mut remainder, b_arr);
            q_arr[limb] |= 1u64 << bit;
        }
    }
}

// ==================== Power ====================

/// Wrapping power: `result = base^exp mod 2^256`.
///
/// Square-and-multiply using `arith256` for each wrapping multiply.
#[no_mangle]
pub unsafe extern "C" fn wpow256_c(base: *const u64, exp: *const u64, result: *mut u64) {
    let base_arr = &*(base as *const [u64; 4]);
    let exp_arr = &*(exp as *const [u64; 4]);
    let r_arr = &mut *(result as *mut [u64; 4]);

    // Handle exp == 0 → result = 1
    if (exp_arr[0] | exp_arr[1] | exp_arr[2] | exp_arr[3]) == 0 {
        *r_arr = ONE;
        return;
    }

    let mut acc = ONE; // accumulator = 1
    let mut b = *base_arr; // current power of base

    for i in 0..4 {
        let mut word = exp_arr[i];
        for _ in 0..64 {
            if word & 1 == 1 {
                acc = wrapping_mul_256(&acc, &b);
            }
            b = wrapping_mul_256(&b, &b);
            word >>= 1;
        }
    }

    *r_arr = acc;
}

// ==================== Internal helpers ====================

/// Wrapping 256×256→256 multiply via `arith256` syscall.
#[inline]
fn wrapping_mul_256(a: &[u64; 4], b: &[u64; 4]) -> [u64; 4] {
    let mut dl = [0u64; 4];
    let mut dh = [0u64; 4];
    let mut params = SyscallArith256Params {
        a,
        b,
        c: &ZERO,
        dl: &mut dl,
        dh: &mut dh,
    };
    syscall_arith256(&mut params);
    dl
}

/// Position of the most significant set bit (0-based). Returns 0 for input 0.
fn msb_pos(x: &[u64; 4]) -> usize {
    for i in (0..4).rev() {
        if x[i] != 0 {
            return i * 64 + (63 - x[i].leading_zeros() as usize);
        }
    }
    0
}

/// Left-shift a 256-bit number by 1 bit in-place.
#[inline]
fn shl1_256(x: &mut [u64; 4]) {
    x[3] = (x[3] << 1) | (x[2] >> 63);
    x[2] = (x[2] << 1) | (x[1] >> 63);
    x[1] = (x[1] << 1) | (x[0] >> 63);
    x[0] <<= 1;
}

/// Compare two 256-bit numbers. Returns -1, 0, or 1.
#[inline]
fn cmp_256(a: &[u64; 4], b: &[u64; 4]) -> i32 {
    for i in (0..4).rev() {
        if a[i] < b[i] {
            return -1;
        }
        if a[i] > b[i] {
            return 1;
        }
    }
    0
}

/// Subtract b from a in-place: `a -= b`. Assumes `a >= b`.
#[inline]
fn sub_256_inplace(a: &mut [u64; 4], b: &[u64; 4]) {
    let mut borrow: u64 = 0;
    for i in 0..4 {
        let (s1, b1) = a[i].overflowing_sub(b[i]);
        let (s2, b2) = s1.overflowing_sub(borrow);
        a[i] = s2;
        borrow = (b1 as u64) + (b2 as u64);
    }
}
