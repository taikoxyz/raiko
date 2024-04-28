use pipeline;

/// Build the example project with both sp1 and risc0 from build.rs
/// Risc0 only works with build.rs
fn main() {
    println!("Hello, world!");

    pipeline::sp1::bins("./", &["example", "foo"]);
    pipeline::sp1::tests("./", &["example", "foo", "bar"]);

    pipeline::risc0::bins(
        "./",
        &["example", "foo"],
        &["methods/example.rs", "methods/foo.rs"],
    );
    pipeline::risc0::tests(
        "./",
        &["example", "foo", "bar"],
        &[
            "methods/test_example.rs",
            "methods/test_foo.rs",
            "methods/test_bar.rs",
        ],
    );
}
