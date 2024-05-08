#![no_main]
#![allow(unused_imports)]
use harness::*;
risc0_zkvm::guest::entry!(main);

use harness::*;

fn main() {
    call_foo();
    #[cfg(test)]
    harness::zk_suits!(test_foo_ok, test_foo_fail);
}

fn call_foo() {
    bar::add(1, 2);
}

#[test]
fn test_foo_ok() {
    harness::assert_eq!(bar::add(1, 2), 3);
}

#[test]
fn test_foo_fail() {
    harness::assert_eq!(bar::add(1, 2), 4);
}
