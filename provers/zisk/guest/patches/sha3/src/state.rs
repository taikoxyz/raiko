use core::convert::TryInto;
#[cfg(feature = "zeroize")]
use zeroize::{Zeroize, ZeroizeOnDrop};

const PLEN: usize = 25;
const DEFAULT_ROUND_COUNT: usize = 24;

#[derive(Clone)]
pub(crate) struct Sha3State {
    pub state: [u64; PLEN],
    round_count: usize,
}

impl Default for Sha3State {
    fn default() -> Self {
        Self {
            state: [0u64; PLEN],
            round_count: DEFAULT_ROUND_COUNT,
        }
    }
}

#[cfg(feature = "zeroize")]
impl Drop for Sha3State {
    fn drop(&mut self) {
        self.state.zeroize();
    }
}

#[cfg(feature = "zeroize")]
impl ZeroizeOnDrop for Sha3State {}

impl Sha3State {
    pub(crate) fn new(round_count: usize) -> Self {
        Self {
            state: [0u64; PLEN],
            round_count,
        }
    }

    #[inline(always)]
    pub(crate) fn absorb_block(&mut self, block: &[u8]) {
        debug_assert_eq!(block.len() % 8, 0);

        for (b, s) in block.chunks_exact(8).zip(self.state.iter_mut()) {
            *s ^= u64::from_le_bytes(b.try_into().unwrap());
        }

        permute(&mut self.state, self.round_count);
    }

    #[inline(always)]
    pub(crate) fn as_bytes(&self, out: &mut [u8]) {
        for (o, s) in out.chunks_mut(8).zip(self.state.iter()) {
            o.copy_from_slice(&s.to_le_bytes()[..o.len()]);
        }
    }

    #[inline(always)]
    pub(crate) fn permute(&mut self) {
        permute(&mut self.state, self.round_count);
    }
}

/// Keccak-f[1600] permutation: on ZisK uses the native syscall precompile,
/// on all other targets falls back to the pure-Rust keccak crate.
#[cfg(all(target_os = "zkvm", target_vendor = "zisk"))]
#[inline(always)]
fn permute(state: &mut [u64; PLEN], _round_count: usize) {
    extern "C" {
        fn syscall_keccak_f(state: *mut u64);
    }
    // SAFETY: state is a valid 25-element u64 array; the ZisK runtime owns the
    // keccak precompile and expects exactly this calling convention.
    unsafe { syscall_keccak_f(state.as_mut_ptr()) };
}

#[cfg(not(all(target_os = "zkvm", target_vendor = "zisk")))]
#[inline(always)]
fn permute(state: &mut [u64; PLEN], round_count: usize) {
    keccak::p1600(state, round_count);
}
