use std::str::FromStr;

use alloy_primitives::Address;
use alloy_sol_types::SolCall;
use once_cell::sync::Lazy;

pub static L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0xcD5e2bebd3DfE46e4BF96aE2ac7B89B22cc6a982")
        .expect("invalid l1 signal service")
});

pub static L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x1000777700000000000000000000000000000007")
        .expect("invalid l2 signal service")
});

pub const ANCHOR_SELECTOR: [u8; 4] = super::anchorCall::SELECTOR;
pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
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
    Address::from_str("0x610178dA211FEF7D417bC0e6FeD39F05609AD788")
        .expect("invalid l1 contract address")
});

pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x1000777700000000000000000000000000000001")
        .expect("invalid l2 contract address")
});
