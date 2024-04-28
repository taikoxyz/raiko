#![no_main]
sp1_zkvm::entrypoint!(main);

use bar;

fn main() {
    call_foo();
}

fn call_foo() {
    bar::add(1, 2);
}
