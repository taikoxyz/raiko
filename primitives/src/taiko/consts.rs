use std::str::FromStr;

use alloy_primitives::{uint, Address, U256};
use once_cell::sync::Lazy;

pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
pub const MAX_TX_LIST_BYTES: usize = 120_000;
pub static BLOCK_GAS_LIMIT: Lazy<U256> = Lazy::new(|| uint!(240250000_U256));
pub static GOLDEN_TOUCH_ACCOUNT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec")
        .expect("invalid golden touch account")
});

pub mod testnet {
    use super::*;
    pub const CHAIN_ID: u64 = 167009;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0xaC6ccC4B3aBA6E96E2F58E0fF7A4ff3aF469E62E")
            .expect("invalid l1 contract address")
    });
    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670090000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });
    pub static SGX_VERIFIER_ADDRESS: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x914e458035Cd10B3650B4115D74f351f79EA768E")
            .expect("invalid sgx verifier contract address")
    });
    pub const GENISES_TIME: u64 = 1695902400u64;
    pub const SECONDS_PER_SLOT: u64 = 12u64;
}

pub mod internal_devnet_a {
    use super::*;
    pub const CHAIN_ID: u64 = 167001;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0xC069c3d2a9f2479F559AD34485698ad5199C555f")
            .expect("invalid l1 contract address")
    });
    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670010000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });
    pub static SGX_VERIFIER_ADDRESS: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x558E38a3286916934Cb63ced04558A52F7Ce67a9")
            .expect("invalid sgx verifier contract address")
    });
    pub const GENISES_TIME: u64 = 1695902400u64;
    pub const SECONDS_PER_SLOT: u64 = 12u64;
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
    pub static SGX_VERIFIER_ADDRESS: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x558E38a3286916934Cb63ced04558A52F7Ce67a9")
            .expect("invalid sgx verifier contract address")
    });
    pub const GENISES_TIME: u64 = 1695902400u64;
    pub const SECONDS_PER_SLOT: u64 = 12u64;
}
