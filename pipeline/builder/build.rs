use pipeline;

#[cfg(feature = "sp1")]
fn main() {
    pipeline::sp1::bins("../example-sp1", &["example", "foo"]);
    pipeline::sp1::tests("../example-sp1", &["example", "bar"]);
}

#[cfg(feature = "risc0")]
fn main() {
    pipeline::risc0::bins(
        "../example-risc0",
        &["example", "foo"],
        &[
            "../example-risc0/methods/example.rs",
            "../example-risc0/methods/foo.rs",
        ],
    );
    pipeline::risc0::tests(
        "../example-risc0",
        &["example", "bar"],
        &[
            "../example-risc0/methods/test_example.rs",
            "../example-risc0/methods/test_bar.rs",
        ],
    );
}

#[cfg(not(any(feature = "sp1", feature = "risc0")))]
fn main() {
    println!("Hello, world!");
}
