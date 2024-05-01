#![no_main]
#![allow(unused_imports)]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);
use harness::*;

fn main() {
    let mut a = 1;
    let mut b = 1;
    for _ in 0..10 {
        let c = a + b;
        a = b;
        b = c;
    }

    #[cfg(test)]
    test_fib();
}

#[test]
fn test_fib() {
    let mut a = 1;
    let mut b = 1;
    for _ in 0..10 {
        let c = a + b;
        a = b;
        b = c;
    }
    harness::assert_eq!(b, 144);
}
