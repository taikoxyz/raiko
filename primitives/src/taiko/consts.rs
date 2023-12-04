use std::str::FromStr;

use alloy_primitives::{uint, Address, U256};
use alloy_sol_types::SolCall;
use once_cell::sync::Lazy;

pub static L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0xf7f1b1Cf92f24aa4BFf028eAAEF15a6159045fC7")
        .expect("invalid l1 signal service")
});

pub static L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x1670010000000000000000000000000000000005")
        .expect("invalid l2 signal service")
});

pub const ANCHOR_SELECTOR: [u8; 4] = super::anchorCall::SELECTOR;
pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
pub const BLOCK_GAS_LIMIT: Lazy<U256> = Lazy::new(|| uint!(15250000_U256));
pub const PROPOSE_BLOCK_SELECTOR: [u8; 4] = super::proposeBlockCall::SELECTOR;

pub static GOLDEN_TOUCH_ACCOUNT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec")
        .expect("invalid golden touch account")
});

pub static TREASURY: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0xdf09A0afD09a63fb04ab3573922437e1e637dE8b")
        .expect("invalid treasury account")
});

pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0xB261C6fACc0707B674c7491338412FDF44733A4B")
        .expect("invalid l1 contract address")
});

pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x1670010000000000000000000000000000010001")
        .expect("invalid l2 contract address")
});

pub static GX1: Lazy<U256> =
    Lazy::new(|| uint!(0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798_U256));
pub static GX2: Lazy<U256> =
    Lazy::new(|| uint!(0xc6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5_U256));

pub const TIER_SGX_ID: u16 = 200;
