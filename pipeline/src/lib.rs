use std::path::PathBuf;

mod builder;
mod executor;
#[cfg(feature = "risc0")]
mod risc0;

use builder::{parse_metadata, GuestBuilder, GuestMetadata};
use cargo_metadata::Metadata;

fn main() {
    println!("Hello, world!");
    sp1_bins("../a", &["bins", "d"]);
}

/// Compile all the Sp1 binaries in the manifest
fn sp1(manifest: &str) {
    let meta = parse_metadata(manifest);
    let bins = meta
        .bins()
        .iter()
        .map(|t| t.name.clone())
        .collect::<Vec<String>>();
    println!("Compiling Sp1 bins: {:?}", bins);
    sp1_inner(meta, &bins);
}

/// Compile the specified Sp1 binaries in the manifest
fn sp1_bins(manifest: &str, bins: &[&str]) {
    let meta = parse_metadata(manifest);
    let bins = meta
        .bins()
        .iter()
        .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
        .map(|t| t.name.clone())
        .collect::<Vec<_>>();

    println!("Compiling Sp1 bins: {:?}", bins);
    sp1_inner(meta, &bins);
}

fn sp1_inner(meta: Metadata, bins: &Vec<String>) {
    let builder = GuestBuilder::new(&meta, "riscv32im-succinct-zkvm-elf", "succinct")
        .rust_flags(&[
            "passes=loweratomic",
            "link-arg=-Ttext=0x00200800",
            "panic=abort",
        ])
        .custom_args(&["--ignore-rust-version"]);
    let executor = builder.build_command("release", bins);

    println!("executor: {:?}", executor);

    let _ = executor
        .execute()
        .expect("Execution failed")
        .sp1_placement(&meta);
}
