use rand_chacha::rand_core::{RngCore, SeedableRng};
use std::mem::MaybeUninit;

static mut RNG: MaybeUninit<rand_chacha::ChaCha8Rng> = MaybeUninit::uninit();

pub unsafe fn init() {
    unsafe {
        RNG.write(rand_chacha::ChaCha8Rng::from_seed(*include_bytes!(
            "../random_seed.bin"
        )));
    }
}

/// Our own random number generator, as powdr doesn't have one.
fn get_random(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    unsafe {
        RNG.assume_init_mut().fill_bytes(buf);
    }

    Ok(())
}

//getrandom::register_custom_getrandom!(get_random);
