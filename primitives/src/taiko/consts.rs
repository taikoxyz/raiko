use std::str::FromStr;

use alloy_primitives::{uint, Address, U256};
use once_cell::sync::Lazy;

pub static L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0xb36a8742CDeA0195f5e659DED165FeA0dCC9F485")
        .expect("invalid l1 signal service")
});

pub static L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x1670080000000000000000000000000000000005")
        .expect("invalid l2 signal service")
});

pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
pub static BLOCK_GAS_LIMIT: Lazy<U256> = Lazy::new(|| uint!(15250000_U256));

pub static GOLDEN_TOUCH_ACCOUNT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec")
        .expect("invalid golden touch account")
});

pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x62Acda3Ad15B382C32B2fB21BEAc9DfB95bbb02F")
        .expect("invalid l1 contract address")
});

pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x1670080000000000000000000000000000010001")
        .expect("invalid l2 contract address")
});
