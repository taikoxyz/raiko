#![no_main]
#![allow(unused_imports)]
risc0_zkvm::guest::entry!(run);
use harness::*;

fn run() {
    #[cfg(test)]
    harness::zk_suits!(test_bar_ok, test_bar_fail);
}

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn test_bar_ok() {
    harness::assert_eq!(add(1, 2), 3);
}

#[test]
fn test_bar_fail() {
    harness::assert_eq!(111, 2222);
}
