#![no_main]
risc0_zkvm::guest::entry!(main);

use bar;

fn main() {
    call_foo();
}

fn call_foo() {
    bar::add(1, 2);
}
