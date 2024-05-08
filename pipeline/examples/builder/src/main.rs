use raiko_pipeline::Pipeline;
mod sp1;
mod risc0;

#[cfg(feature = "sp1")]
fn main() {
    let pipeline = sp1::Sp1Pipeline::new("../sp1", "release");
    pipeline.bins(&["example", "foo"], "../sp1/elf");
    pipeline.tests(&["example", "kzg"], "../sp1/elf");
}

#[cfg(feature = "risc0")]
fn main() {
    let pipeline = risc0::Risc0Pipeline::new("../risc0", "release");
    pipeline.bins(&["example", "foo-foo"], "../risc0/methods/src");
    pipeline.tests(&["example", "kzg"], "../risc0/methods/src");
}

#[cfg(not(any(feature = "sp1", feature = "risc0")))]
fn main() {
    println!("Please use --features to specify the target platform.");
}
