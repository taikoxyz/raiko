use crate::{maybe_prove, Risc0Param};
use bincode::config;
use raiko_lib::input::{GuestInput, GuestOutput};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

#[test]
fn test_guest_list() {
    println!("elf code length: {}", RISC0_METHODS_TEST_ELF.len());

    let config = Risc0Param::default();

    let result = maybe_prove::<Vec<String>, ()>(
        &config,
        Vec::new(),
        RISC0_METHODS_TEST_ELF,
        &(),
        Default::default(),
    );
}
