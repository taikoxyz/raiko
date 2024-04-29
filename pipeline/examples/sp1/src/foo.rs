#![no_main]
sp1_zkvm::entrypoint!(main);
use harness::*;

fn main() {
    call_foo();

    #[cfg(test)]
    harness::zk_suits!(test_call_foo, test_call_foo_fail);
}

fn call_foo() -> i32 {
    let x = 1;
    let mut sum = 0;
    for _ in 0..4 {
        sum = bar::add(sum, x);
    }
    sum
}

#[test]
fn test_call_foo() {
    assert_eq!(call_foo(), 10);
}

#[test]
fn test_call_foo_fail() {
    assert_eq!(call_foo(), 9999);
}
