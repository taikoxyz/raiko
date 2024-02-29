use std::str::FromStr;

use alloy_primitives::{uint, Address, U256};
use once_cell::sync::Lazy;

pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
pub const MAX_TX_LIST: usize = 149;
pub const MAX_TX_LIST_BYTES: usize = 120_000;
pub static BLOCK_GAS_LIMIT: Lazy<U256> = Lazy::new(|| uint!(15250000_U256));
pub static GOLDEN_TOUCH_ACCOUNT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec")
        .expect("invalid golden touch account")
});
pub static SGX_VERIFIER_ADDRESS: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0xfb910865585308c1Ff916c6d97Aa3198098039C2")
        .expect("invalid sgx verifier contract address")
});

pub mod testnet {
    use super::*;
    pub const CHAIN_ID: u64 = 167008;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0xB20BB9105e007Bd3E0F73d63D4D3dA2c8f736b77")
            .expect("invalid l1 contract address")
    });
    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670080000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });
}

pub mod internal_devnet_a {
    use super::*;
    pub const CHAIN_ID: u64 = 167001;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1FD3Df7E9C15390c8589D2E4d43757eA692ae256")
            .expect("invalid l1 contract address")
    });
    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670010000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });
}

pub mod internal_devnet_b {
    use super::*;
    pub const CHAIN_ID: u64 = 167002;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x674313F932cc0cE272154a288cf3De474D44e14F")
            .expect("invalid l1 contract address")
    });
    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670020000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });
}
