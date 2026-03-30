//! Shims bridging zisk-0.15.0 patched crates → ziskos 0.16.0 syscall API.
//!
//! Field/scalar arithmetic uses `syscall_arith256_mod` (d = a·b + c mod m).
//! EC point operations use **native** `syscall_secp256k1_add` / `syscall_secp256k1_dbl`
//! syscalls — one syscall per EC add/double instead of 13-17 arith_mod calls.
//! This reduces total ecrecover cost by ~78%.

use ziskos::syscalls::{
    syscall_arith256_mod, syscall_secp256k1_add, syscall_secp256k1_dbl, syscall_sha256_f,
    SyscallArith256ModParams, SyscallPoint256, SyscallSecp256k1AddParams, SyscallSha256Params,
};

// ========================= Constants =========================

/// secp256k1 base-field prime p (little-endian u64 limbs).
const P: [u64; 4] = [
    0xFFFFFFFE_FFFFFC2F,
    0xFFFFFFFF_FFFFFFFF,
    0xFFFFFFFF_FFFFFFFF,
    0xFFFFFFFF_FFFFFFFF,
];

/// p − 1
const P_MINUS_ONE: [u64; 4] = [
    0xFFFFFFFE_FFFFFC2E,
    0xFFFFFFFF_FFFFFFFF,
    0xFFFFFFFF_FFFFFFFF,
    0xFFFFFFFF_FFFFFFFF,
];

/// (p + 1) / 4  (square-root exponent; p ≡ 3 mod 4).
const P_PLUS_ONE_DIV_4: [u64; 4] = [
    0xFFFFFFFF_BFFFFF0C,
    0xFFFFFFFF_FFFFFFFF,
    0xFFFFFFFF_FFFFFFFF,
    0x3FFFFFFF_FFFFFFFF,
];

/// secp256k1 scalar-field order n.
const N: [u64; 4] = [
    0xBFD25E8C_D0364141,
    0xBAAEDCE6_AF48A03B,
    0xFFFFFFFF_FFFFFFFE,
    0xFFFFFFFF_FFFFFFFF,
];

/// n − 1
const N_MINUS_ONE: [u64; 4] = [
    0xBFD25E8C_D0364140,
    0xBAAEDCE6_AF48A03B,
    0xFFFFFFFF_FFFFFFFE,
    0xFFFFFFFF_FFFFFFFF,
];

/// n − 2  (Fermat inverse exponent for the scalar field).
const N_MINUS_TWO: [u64; 4] = [
    0xBFD25E8C_D036413F,
    0xBAAEDCE6_AF48A03B,
    0xFFFFFFFF_FFFFFFFE,
    0xFFFFFFFF_FFFFFFFF,
];

const ONE: [u64; 4] = [1, 0, 0, 0];
const ZERO_256: [u64; 4] = [0; 4];

/// Curve equation constant b: y² = x³ + 7
const E_B: [u64; 4] = [7, 0, 0, 0];

/// Generator x-coordinate (little-endian u64).
const G_X: [u64; 4] = [
    0x59F2815B_16F81798,
    0x029BFCDB_2DCE28D9,
    0x55A06295_CE870B07,
    0x79BE667E_F9DCBBAC,
];

/// Generator y-coordinate (little-endian u64).
const G_Y: [u64; 4] = [
    0x9C47D08F_FB10D4B8,
    0xFD17B448_A6855419,
    0x5DA4FBFC_0E1108A8,
    0x483ADA77_26A3C465,
];

// ========================= SHA-256 shim =========================

/// Signature expected by zisk-patch-hashes 0.15.0:
///   `sha256f_compress_c(state: *mut u32, blocks: *const u8, num_blocks: usize)`
///
/// Delegates to ziskos 0.16.0 `syscall_sha256_f` one block at a time.
#[no_mangle]
pub unsafe extern "C" fn sha256f_compress_c(
    state_ptr: *mut u32,
    blocks_ptr: *const u8,
    num_blocks: usize,
) {
    let state = &mut *(state_ptr as *mut [u64; 4]);
    for i in 0..num_blocks {
        let block = &*(blocks_ptr.add(i * 64) as *const [u64; 8]);
        let mut params = SyscallSha256Params {
            state,
            input: block,
        };
        syscall_sha256_f(&mut params);
    }
}

// ========================= Internal helpers =========================

/// d = (a · b + c) mod m  via a single `arith256_mod` syscall.
#[inline]
fn arith_mod(a: &[u64; 4], b: &[u64; 4], c: &[u64; 4], m: &[u64; 4]) -> [u64; 4] {
    let mut d = [0u64; 4];
    let mut params = SyscallArith256ModParams {
        a,
        b,
        c,
        module: m,
        d: &mut d,
    };
    syscall_arith256_mod(&mut params);
    d
}

/// Modular exponentiation via binary square-and-multiply (LSB-first).
///
/// Returns `base^exp mod modulus`.
fn mod_pow(base: &[u64; 4], exp: &[u64; 4], modulus: &[u64; 4]) -> [u64; 4] {
    let mut result = ONE;
    let mut b = *base;
    for i in 0..4 {
        for bit in 0..64 {
            if (exp[i] >> bit) & 1 == 1 {
                result = arith_mod(&result, &b, &ZERO_256, modulus);
            }
            b = arith_mod(&b, &b, &ZERO_256, modulus);
        }
    }
    result
}

/// Field multiplicative inverse: a^(p−2) mod p.
///
/// Uses a dedicated addition chain (from libsecp256k1) that exploits the
/// special structure of p = 2^256 − 2^32 − 977 to compute the inverse
/// in 270 arith_mod syscalls instead of the ~505 required by generic
/// square-and-multiply.
fn fp_inv(a: &[u64; 4]) -> [u64; 4] {
    // Build a^(2^k - 1) for k ∈ {2,3,6,9,11,22,44,88,176,220,223}
    let x2 = fp_mul(&fp_sqr(a), a); // a^(2^2  - 1)
    let x3 = fp_mul(&fp_sqr(&x2), a); // a^(2^3  - 1)
    let x6 = fp_mul(&fp_sqr_n(&x3, 3), &x3); // a^(2^6  - 1)
    let x9 = fp_mul(&fp_sqr_n(&x6, 3), &x3); // a^(2^9  - 1)
    let x11 = fp_mul(&fp_sqr_n(&x9, 2), &x2); // a^(2^11 - 1)
    let x22 = fp_mul(&fp_sqr_n(&x11, 11), &x11); // a^(2^22 - 1)
    let x44 = fp_mul(&fp_sqr_n(&x22, 22), &x22); // a^(2^44 - 1)
    let x88 = fp_mul(&fp_sqr_n(&x44, 44), &x44); // a^(2^88 - 1)
    let x176 = fp_mul(&fp_sqr_n(&x88, 88), &x88); // a^(2^176- 1)
    let x220 = fp_mul(&fp_sqr_n(&x176, 44), &x44); // a^(2^220- 1)
    let x223 = fp_mul(&fp_sqr_n(&x220, 3), &x3); // a^(2^223- 1)

    // Assemble p−2 = [223 ones] 0 [22 ones] 0000 1 0 11 0 1
    let t = fp_mul(&fp_sqr_n(&x223, 23), &x22); // + 0 [22 ones]
    let t = fp_mul(&fp_sqr_n(&t, 5), a); // + 0000 1
    let t = fp_mul(&fp_sqr_n(&t, 3), &x2); // + 0 11
    fp_mul(&fp_sqr_n(&t, 2), a) // + 0 1
}

/// Scalar multiplicative inverse: a^(n−2) mod n.
#[inline]
fn fn_inv_internal(a: &[u64; 4]) -> [u64; 4] {
    mod_pow(a, &N_MINUS_TWO, &N)
}

/// Modular square root: a^((p+1)/4) mod p.
#[inline]
fn fp_sqrt(a: &[u64; 4]) -> [u64; 4] {
    mod_pow(a, &P_PLUS_ONE_DIV_4, &P)
}

#[inline]
fn is_zero_256(x: &[u64; 4]) -> bool {
    (x[0] | x[1] | x[2] | x[3]) == 0
}

#[inline]
fn eq_256(a: &[u64; 4], b: &[u64; 4]) -> bool {
    a[0] == b[0] && a[1] == b[1] && a[2] == b[2] && a[3] == b[3]
}

/// Returns `(limb_index, bit_index)` of the most significant set bit.
/// Panics on zero.
fn msb_position(x: &[u64; 4]) -> (usize, usize) {
    for i in (0..4).rev() {
        if x[i] != 0 {
            return (i, 63 - x[i].leading_zeros() as usize);
        }
    }
    panic!("msb_position: zero input");
}

/// Maximum MSB position across two non-zero-together values.
fn msb_position_max(a: &[u64; 4], b: &[u64; 4]) -> (usize, usize) {
    let (al, ab) = if is_zero_256(a) {
        (0usize, 0usize)
    } else {
        msb_position(a)
    };
    let (bl, bb) = if is_zero_256(b) {
        (0usize, 0usize)
    } else {
        msb_position(b)
    };
    if al > bl || (al == bl && ab >= bb) {
        (al, ab)
    } else {
        (bl, bb)
    }
}

/// Convert 32 big-endian bytes → 4 little-endian u64 limbs.
fn bytes_be_to_u64_le(bytes: &[u8]) -> [u64; 4] {
    let mut r = [0u64; 4];
    for i in 0..4 {
        for j in 0..8 {
            r[3 - i] |= (bytes[i * 8 + j] as u64) << (8 * (7 - j));
        }
    }
    r
}

/// Field subtraction: (a - b) mod p.
#[inline]
#[allow(dead_code)]
fn fp_sub(a: &[u64; 4], b: &[u64; 4]) -> [u64; 4] {
    arith_mod(b, &P_MINUS_ONE, a, &P)
}

/// Field multiplication: (a * b) mod p.
#[inline]
fn fp_mul(a: &[u64; 4], b: &[u64; 4]) -> [u64; 4] {
    arith_mod(a, b, &ZERO_256, &P)
}

/// Field squaring: a² mod p.
#[inline]
fn fp_sqr(a: &[u64; 4]) -> [u64; 4] {
    arith_mod(a, a, &ZERO_256, &P)
}

/// Repeated squaring: a^(2^n) mod p.
#[inline]
fn fp_sqr_n(a: &[u64; 4], n: usize) -> [u64; 4] {
    let mut r = *a;
    for _ in 0..n {
        r = fp_sqr(&r);
    }
    r
}

// =================== Native EC syscall wrappers ====================

/// Convert our [u64; 8] affine representation to SyscallPoint256.
#[inline]
#[allow(dead_code)]
fn to_syscall_point(p: &[u64; 8]) -> SyscallPoint256 {
    SyscallPoint256 {
        x: [p[0], p[1], p[2], p[3]],
        y: [p[4], p[5], p[6], p[7]],
    }
}

/// Convert SyscallPoint256 back to our [u64; 8] affine representation.
#[inline]
fn from_syscall_point(p: &SyscallPoint256) -> [u64; 8] {
    [
        p.x[0], p.x[1], p.x[2], p.x[3], p.y[0], p.y[1], p.y[2], p.y[3],
    ]
}

/// Check if a SyscallPoint256 is the identity (point at infinity).
/// Convention: (0, 0) represents the identity.
#[inline]
fn syscall_point_is_identity(p: &SyscallPoint256) -> bool {
    is_zero_256(&p.x) && is_zero_256(&p.y)
}

/// EC point addition via native `syscall_secp256k1_add`.
/// Result stored in p1 (mutated in-place). Returns the result.
#[inline]
fn native_ec_add(p1: &mut SyscallPoint256, p2: &SyscallPoint256) {
    let mut params = SyscallSecp256k1AddParams { p1, p2 };
    syscall_secp256k1_add(&mut params);
}

/// EC point doubling via native `syscall_secp256k1_dbl`.
/// Result stored in p1 (mutated in-place).
#[inline]
fn native_ec_dbl(p1: &mut SyscallPoint256) {
    syscall_secp256k1_dbl(p1);
}

/// EC point addition in affine coordinates using native syscall.
/// Returns result as [u64; 8].
fn ec_add_affine(p1x: &[u64; 4], p1y: &[u64; 4], p2x: &[u64; 4], p2y: &[u64; 4]) -> [u64; 8] {
    let mut pt1 = SyscallPoint256 { x: *p1x, y: *p1y };
    let pt2 = SyscallPoint256 { x: *p2x, y: *p2y };
    native_ec_add(&mut pt1, &pt2);
    from_syscall_point(&pt1)
}

/// Scalar subtraction in the scalar field: (x − y) mod n.
fn fn_sub_internal(x: &[u64; 4], y: &[u64; 4]) -> [u64; 4] {
    arith_mod(y, &N_MINUS_ONE, x, &N)
}

/// Scalar multiplication: k · P.  Returns `None` when the result is identity.
///
/// Uses native `syscall_secp256k1_dbl` / `syscall_secp256k1_add` — one
/// syscall per EC operation instead of 13-17 arith_mod calls each.
fn scalar_mul_internal(k: &[u64; 4], p: &[u64; 8]) -> Option<[u64; 8]> {
    if is_zero_256(k) {
        return None;
    }

    let (max_limb, max_bit) = msb_position(k);

    // k == 1 → just return P
    if max_limb == 0 && max_bit == 0 {
        return Some(*p);
    }

    let base_x = [p[0], p[1], p[2], p[3]];
    let base_y = [p[4], p[5], p[6], p[7]];

    // Accumulator starts at P (MSB is always 1)
    let mut acc = SyscallPoint256 {
        x: base_x,
        y: base_y,
    };
    let mut is_id = false;
    let msb_pos = max_limb * 64 + max_bit;

    for bit_idx in (0..msb_pos).rev() {
        let limb = bit_idx / 64;
        let bit = bit_idx % 64;

        // Double
        if !is_id {
            native_ec_dbl(&mut acc);
            is_id = syscall_point_is_identity(&acc);
        }

        // Conditionally add P
        if (k[limb] >> bit) & 1 == 1 {
            if is_id {
                acc = SyscallPoint256 {
                    x: base_x,
                    y: base_y,
                };
                is_id = false;
            } else {
                let pt = SyscallPoint256 {
                    x: base_x,
                    y: base_y,
                };
                native_ec_add(&mut acc, &pt);
                is_id = syscall_point_is_identity(&acc);
            }
        }
    }

    if is_id {
        None
    } else {
        Some(from_syscall_point(&acc))
    }
}

/// Double scalar multiplication  k1·G + k2·P  (Shamir's trick).
///
/// Lookup table {G, P, G+P} is kept as SyscallPoint256; accumulator runs
/// with native `syscall_secp256k1_dbl` / `syscall_secp256k1_add`.
fn double_scalar_mul_internal(k1: &[u64; 4], k2: &[u64; 4], p: &[u64; 8]) -> Option<[u64; 8]> {
    if is_zero_256(k1) && is_zero_256(k2) {
        return None;
    }
    if is_zero_256(k1) {
        return scalar_mul_internal(k2, p);
    }
    if is_zero_256(k2) {
        let g = [
            G_X[0], G_X[1], G_X[2], G_X[3], G_Y[0], G_Y[1], G_Y[2], G_Y[3],
        ];
        return scalar_mul_internal(k1, &g);
    }

    let px = [p[0], p[1], p[2], p[3]];
    let py = [p[4], p[5], p[6], p[7]];

    // Handle degenerate cases where P shares the same x-coordinate as G.
    if eq_256(&G_X, &px) {
        if eq_256(&G_Y, &py) {
            // P == G → (k1+k2)·G
            let sum = arith_mod(k1, &ONE, k2, &N);
            let g = [
                G_X[0], G_X[1], G_X[2], G_X[3], G_Y[0], G_Y[1], G_Y[2], G_Y[3],
            ];
            return scalar_mul_internal(&sum, &g);
        } else {
            // P == −G → (k1−k2)·G
            let diff = fn_sub_internal(k1, k2);
            let g = [
                G_X[0], G_X[1], G_X[2], G_X[3], G_Y[0], G_Y[1], G_Y[2], G_Y[3],
            ];
            return scalar_mul_internal(&diff, &g);
        }
    }

    // Precompute G + P via native syscall.
    let gp_aff = ec_add_affine(&G_X, &G_Y, &px, &py);

    // Both scalars == 1 → G + P
    if eq_256(k1, &ONE) && eq_256(k2, &ONE) {
        return Some(gp_aff);
    }

    // Precomputed table stored as coordinate pairs (SyscallPoint256 is !Copy)
    let gp_x = [gp_aff[0], gp_aff[1], gp_aff[2], gp_aff[3]];
    let gp_y = [gp_aff[4], gp_aff[5], gp_aff[6], gp_aff[7]];

    // ---------- Shamir's trick: native accumulator ----------
    let (max_limb, max_bit) = msb_position_max(k1, k2);
    let k1_msb = (k1[max_limb] >> max_bit) & 1;
    let k2_msb = (k2[max_limb] >> max_bit) & 1;

    let mut acc = SyscallPoint256 {
        x: ZERO_256,
        y: ZERO_256,
    };
    let mut is_id = true;
    match (k1_msb, k2_msb) {
        (0, 1) => {
            acc = SyscallPoint256 { x: px, y: py };
            is_id = false;
        }
        (1, 0) => {
            acc = SyscallPoint256 { x: G_X, y: G_Y };
            is_id = false;
        }
        (1, 1) => {
            acc = SyscallPoint256 { x: gp_x, y: gp_y };
            is_id = false;
        }
        _ => {}
    }

    let msb_pos = max_limb * 64 + max_bit;
    for bit_idx in (0..msb_pos).rev() {
        let limb = bit_idx / 64;
        let bit = bit_idx % 64;
        let k1_b = (k1[limb] >> bit) & 1;
        let k2_b = (k2[limb] >> bit) & 1;

        // Double the accumulator
        if !is_id {
            native_ec_dbl(&mut acc);
            is_id = syscall_point_is_identity(&acc);
        }

        // Add table entry for this bit-pair
        match (k1_b, k2_b) {
            (0, 0) => { /* nothing */ }
            (0, 1) => {
                if is_id {
                    acc = SyscallPoint256 { x: px, y: py };
                    is_id = false;
                } else {
                    let pt = SyscallPoint256 { x: px, y: py };
                    native_ec_add(&mut acc, &pt);
                    is_id = syscall_point_is_identity(&acc);
                }
            }
            (1, 0) => {
                if is_id {
                    acc = SyscallPoint256 { x: G_X, y: G_Y };
                    is_id = false;
                } else {
                    let pt = SyscallPoint256 { x: G_X, y: G_Y };
                    native_ec_add(&mut acc, &pt);
                    is_id = syscall_point_is_identity(&acc);
                }
            }
            (1, 1) => {
                if is_id {
                    acc = SyscallPoint256 { x: gp_x, y: gp_y };
                    is_id = false;
                } else {
                    let pt = SyscallPoint256 { x: gp_x, y: gp_y };
                    native_ec_add(&mut acc, &pt);
                    is_id = syscall_point_is_identity(&acc);
                }
            }
            _ => unreachable!(),
        }
    }

    if is_id {
        None
    } else {
        Some(from_syscall_point(&acc))
    }
}

/// Standard ECDSA verification.
fn ecdsa_verify_internal(pk: &[u64; 8], z: &[u64; 4], r: &[u64; 4], s: &[u64; 4]) -> bool {
    if is_zero_256(r) || is_zero_256(s) {
        return false;
    }

    let s_inv = fn_inv_internal(s);
    let u1 = arith_mod(z, &s_inv, &ZERO_256, &N);
    let u2 = arith_mod(r, &s_inv, &ZERO_256, &N);

    match double_scalar_mul_internal(&u1, &u2, pk) {
        None => false,
        Some(r_point) => {
            let rx = [r_point[0], r_point[1], r_point[2], r_point[3]];
            let rx_mod_n = arith_mod(&rx, &ONE, &ZERO_256, &N);
            eq_256(&rx_mod_n, r)
        }
    }
}

// ========================= secp256k1 field ops (mod P) =========================

/// d = (x · 1 + 0) mod P — field reduction.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fp_reduce_c(x_ptr: *const u64, out_ptr: *mut u64) {
    let x = &*(x_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, &ONE, &ZERO_256, &P);
}

/// d = (x · 1 + y) mod P — field addition.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fp_add_c(
    x_ptr: *const u64,
    y_ptr: *const u64,
    out_ptr: *mut u64,
) {
    let x = &*(x_ptr as *const [u64; 4]);
    let y = &*(y_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, &ONE, y, &P);
}

/// d = x · (P−1) mod P  =  −x mod P — field negation.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fp_negate_c(x_ptr: *const u64, out_ptr: *mut u64) {
    let x = &*(x_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, &P_MINUS_ONE, &ZERO_256, &P);
}

/// d = (x · y + 0) mod P — field multiplication.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fp_mul_c(
    x_ptr: *const u64,
    y_ptr: *const u64,
    out_ptr: *mut u64,
) {
    let x = &*(x_ptr as *const [u64; 4]);
    let y = &*(y_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, y, &ZERO_256, &P);
}

/// d = (x · scalar + 0) mod P — field scalar multiplication.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fp_mul_scalar_c(
    x_ptr: *const u64,
    scalar: u64,
    out_ptr: *mut u64,
) {
    let x = &*(x_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    let s = [scalar, 0, 0, 0];
    *out = arith_mod(x, &s, &ZERO_256, &P);
}

// ========================= secp256k1 scalar ops (mod N) =========================

/// d = (x · 1 + 0) mod N — scalar reduction.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fn_reduce_c(x_ptr: *const u64, out_ptr: *mut u64) {
    let x = &*(x_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, &ONE, &ZERO_256, &N);
}

/// d = (x · 1 + y) mod N — scalar addition.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fn_add_c(
    x_ptr: *const u64,
    y_ptr: *const u64,
    out_ptr: *mut u64,
) {
    let x = &*(x_ptr as *const [u64; 4]);
    let y = &*(y_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, &ONE, y, &N);
}

/// d = x · (N−1) mod N  =  −x mod N — scalar negation.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fn_neg_c(x_ptr: *const u64, out_ptr: *mut u64) {
    let x = &*(x_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, &N_MINUS_ONE, &ZERO_256, &N);
}

/// d = y · (N−1) + x  mod N  =  x − y  mod N — scalar subtraction.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fn_sub_c(
    x_ptr: *const u64,
    y_ptr: *const u64,
    out_ptr: *mut u64,
) {
    let x = &*(x_ptr as *const [u64; 4]);
    let y = &*(y_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(y, &N_MINUS_ONE, x, &N);
}

/// d = (x · y + 0) mod N — scalar multiplication.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fn_mul_c(
    x_ptr: *const u64,
    y_ptr: *const u64,
    out_ptr: *mut u64,
) {
    let x = &*(x_ptr as *const [u64; 4]);
    let y = &*(y_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = arith_mod(x, y, &ZERO_256, &N);
}

/// Scalar inverse via Fermat's little theorem: x^(n−2) mod n.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_fn_inv_c(x_ptr: *const u64, out_ptr: *mut u64) {
    let x = &*(x_ptr as *const [u64; 4]);
    let out = &mut *(out_ptr as *mut [u64; 4]);
    *out = fn_inv_internal(x);
}

// ========================= secp256k1 curve ops =========================

/// Projective → affine conversion.
///
/// k256 uses **standard** (homogeneous) projective coordinates:
///   affine = (X/Z, Y/Z)
///
/// Input:  12 u64 limbs  [X(4), Y(4), Z(4)]
/// Output:  8 u64 limbs  [x(4), y(4)]
#[no_mangle]
pub unsafe extern "C" fn secp256k1_to_affine_c(p_ptr: *const u64, out_ptr: *mut u64) {
    let px = &*(p_ptr as *const [u64; 4]);
    let py = &*((p_ptr.add(4)) as *const [u64; 4]);
    let pz = &*((p_ptr.add(8)) as *const [u64; 4]);

    let z_inv = fp_inv(pz);

    let out_x = &mut *(out_ptr as *mut [u64; 4]);
    *out_x = arith_mod(px, &z_inv, &ZERO_256, &P);

    let out_y = &mut *((out_ptr.add(4)) as *mut [u64; 4]);
    *out_y = arith_mod(py, &z_inv, &ZERO_256, &P);
}

/// Decompress a secp256k1 point from its x-coordinate (32 big-endian bytes)
/// and a parity flag.  Returns 1 on success, 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_decompress_c(
    x_bytes_ptr: *const u8,
    y_is_odd: u8,
    out_ptr: *mut u64,
) -> u8 {
    let x_bytes = core::slice::from_raw_parts(x_bytes_ptr, 32);
    let x = bytes_be_to_u64_le(x_bytes);

    // y² = x³ + 7
    let x_sq = arith_mod(&x, &x, &ZERO_256, &P);
    let x_cb = arith_mod(&x_sq, &x, &ZERO_256, &P);
    let y_sq = arith_mod(&x_cb, &ONE, &E_B, &P);

    // Candidate y = y_sq^((p+1)/4) mod p
    let y = fp_sqrt(&y_sq);

    // Verify: y² must equal y_sq (otherwise not a quadratic residue)
    let check = arith_mod(&y, &y, &ZERO_256, &P);
    if !eq_256(&check, &y_sq) {
        return 0;
    }

    // Fix parity
    let parity = (y[0] & 1) as u8;
    let final_y = if parity != y_is_odd {
        arith_mod(&y, &P_MINUS_ONE, &ZERO_256, &P)
    } else {
        y
    };

    let out = core::slice::from_raw_parts_mut(out_ptr, 8);
    out[0..4].copy_from_slice(&x);
    out[4..8].copy_from_slice(&final_y);
    1
}

/// Double scalar multiplication:  k1·G + k2·P.
///
/// Returns `true` when the result **is** the point at infinity (identity),
/// `false` when the output buffer contains a valid affine point.
///
/// This matches the convention expected by the k256 `lincomb()` caller
/// which names the return value `is_identity`.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_double_scalar_mul_with_g_c(
    k1_ptr: *const u64,
    k2_ptr: *const u64,
    p_ptr: *const u64,
    out_ptr: *mut u64,
) -> bool {
    let k1 = &*(k1_ptr as *const [u64; 4]);
    let k2 = &*(k2_ptr as *const [u64; 4]);
    let p = &*(p_ptr as *const [u64; 8]);

    match double_scalar_mul_internal(k1, k2, p) {
        Some(result) => {
            let out = &mut *(out_ptr as *mut [u64; 8]);
            *out = result;
            false // NOT identity — output buffer is valid
        }
        None => true, // IS identity
    }
}

/// ECDSA verification.
///
/// Returns `true` when signature `(r, s)` over message hash `z` is valid for
/// public key `pk`.
#[no_mangle]
pub unsafe extern "C" fn secp256k1_ecdsa_verify_c(
    pk_ptr: *const u64,
    z_ptr: *const u64,
    r_ptr: *const u64,
    s_ptr: *const u64,
) -> bool {
    let pk = &*(pk_ptr as *const [u64; 8]);
    let z = &*(z_ptr as *const [u64; 4]);
    let r = &*(r_ptr as *const [u64; 4]);
    let s = &*(s_ptr as *const [u64; 4]);

    ecdsa_verify_internal(pk, z, r, s)
}
