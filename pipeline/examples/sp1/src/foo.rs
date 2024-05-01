#![no_main]
sp1_zkvm::entrypoint!(main);
use harness::*;

fn main() {
    call_foo();

    #[cfg(test)]
    harness::zk_suits!(test_call_foo, test_call_foo_fail);
}

fn call_foo() -> i32 {
    let mut sum = 0;
    for i in 0..4 {
        sum = bar::add(sum, i);
    }
    sum
}

#[test]
fn test_call_foo() {
    harness::assert_eq!(call_foo(), 6);
}

#[test]
fn test_call_foo_fail() {
    harness::assert_eq!(call_foo(), 9999);
}
