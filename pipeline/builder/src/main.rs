use pipeline;

/// Build the example project with both sp1 and risc0 from build.rs
/// Risc0 only works with build.rs
fn main() {
    println!("Hello, world!");

    // pipeline::sp1::bins("../example", &["example", "foo"]);
    // pipeline::sp1::tests("../example", &["example", "foo", "bar"]);
}
