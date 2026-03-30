// Keccak patch for ZisK: routes keccakf permutation through the ZisK
// `syscall_keccak_f` precompile on zkVM targets, falling back to the
// vanilla Rust implementation everywhere else.

const PLEN: usize = 25;

/// Keccak-p[1600, round_count] permutation.
/// On ZisK, delegates to the native precompile (always 24 rounds).
#[inline(always)]
pub fn p1600(state: &mut [u64; PLEN], _round_count: usize) {
    keccakf(state);
}

/// Keccak-f[1600] permutation (24 rounds).
#[inline(always)]
pub fn f1600(state: &mut [u64; PLEN]) {
    keccakf(state);
}

#[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
#[inline(always)]
fn keccakf(state: &mut [u64; PLEN]) {
    extern "C" {
        fn syscall_keccak_f(state: *mut u64);
    }
    unsafe { syscall_keccak_f(state.as_mut_ptr()) };
}

#[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
#[inline(always)]
fn keccakf(state: &mut [u64; PLEN]) {
    // Vanilla Keccak-f[1600] — 24-round reference implementation.
    const RC: [u64; 24] = [
        0x0000000000000001, 0x0000000000008082, 0x800000000000808A, 0x8000000080008000,
        0x000000000000808B, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
        0x000000000000008A, 0x0000000000000088, 0x0000000080008009, 0x000000008000000A,
        0x000000008000808B, 0x800000000000008B, 0x8000000000008089, 0x8000000000008003,
        0x8000000000008002, 0x8000000000000080, 0x000000000000800A, 0x800000008000000A,
        0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
    ];
    const RHO: [u32; 24] = [
        1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44,
    ];
    const PI: [usize; 24] = [
        10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1,
    ];

    let s = state;
    let mut array = [0u64; 5];

    for rc in RC.iter() {
        // Theta
        for x in 0..5 {
            array[x] = s[x] ^ s[x + 5] ^ s[x + 10] ^ s[x + 15] ^ s[x + 20];
        }
        for x in 0..5 {
            let t = array[(x + 4) % 5] ^ array[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                s[x + y * 5] ^= t;
            }
        }
        // Rho + Pi
        let mut last = s[1];
        for (i, &pi) in PI.iter().enumerate() {
            let tmp = s[pi];
            s[pi] = last.rotate_left(RHO[i]);
            last = tmp;
        }
        // Chi
        for y in 0..5 {
            let off = y * 5;
            array[..5].copy_from_slice(&s[off..off + 5]);
            for x in 0..5 {
                s[off + x] = array[x] ^ ((!array[(x + 1) % 5]) & array[(x + 2) % 5]);
            }
        }
        // Iota
        s[0] ^= rc;
    }
}
