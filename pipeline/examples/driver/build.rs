#[cfg(feature = "sp1")]
fn main() {
    pipeline::sp1::bins("../sp1", &["example", "foo"]);
    pipeline::sp1::tests("../sp1", &["example", "foo"]);
}

#[cfg(feature = "risc0")]
fn main() {
    pipeline::risc0::bins(
        "../risc0",
        &["example", "foo"],
        &["../risc0/methods/example.rs", "../risc0/methods/foo.rs"],
    );
    pipeline::risc0::tests(
        "../risc0",
        &["example", "bar"],
        &[
            "../risc0/methods/test_example.rs",
            "../risc0/methods/test_bar.rs",
        ],
    );
}

#[cfg(not(any(feature = "sp1", feature = "risc0")))]
fn main() {
    println!("Hello, world!");
}
