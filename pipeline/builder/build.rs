use pipeline;

/// Build the example project with both sp1 and risc0 from build.rs
/// Risc0 only works with build.rs
fn main() {
    println!("Hello, world!");
    pipeline::risc0::bins(
        "../example",
        &["example", "foo"],
        &["../example/methods/example.rs", "../example/methods/foo.rs"],
    );
    pipeline::risc0::tests(
        "../example",
        &["example", "bar"],
        &[
            "../example/methods/test_example.rs",
            "../example/methods/test_bar.rs",
        ],
    );
}
