use raiko_pipeline::Pipeline;

#[cfg(feature = "sp1")]
fn main() {
    let pipeline = raiko_pipeline::Sp1Pipeline::new("../sp1", "release");
    pipeline.bins(&["example", "foo"], "../sp1/elf");
    pipeline.tests(&["example", "foo"], "../sp1/elf");
}

#[cfg(feature = "risc0")]
fn main() {
    let pipeline = raiko_pipeline::risc0::Risc0Pipeline::new("../risc0", "release");
    pipeline.bins(&["example", "foo-foo"], "../risc0/methods");
    pipeline.tests(&["example", "foo-foo"], "../risc0/methodsf");
}

#[cfg(not(any(feature = "sp1", feature = "risc0")))]
fn main() {
    println!("Please use --features to specify the target platform.");
}
