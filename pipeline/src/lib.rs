mod builder;
mod executor;
#[cfg(feature = "risc0")]
mod risc0_util;

use std::path::PathBuf;

use builder::{parse_metadata, GuestBuilder, GuestMetadata};
use cargo_metadata::Metadata;

fn main() {
    println!("Hello, world!");
    sp1::bins("../a", &["bins", "d"]);
}

#[cfg(feature = "sp1")]
pub mod sp1 {
    use super::*;

    /// Compile the specified Sp1 binaries in the project
    pub fn bins(project: &str, bins: &[&str]) {
        let meta = parse_metadata(project);
        let bins = meta
            .bins()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Sp1 bins: {:?}", bins);
        inner(meta, &bins, false, "release");
    }

    /// Compile the specified Sp1 test in the project
    pub fn tests(project: &str, bins: &[&str]) {
        let meta = parse_metadata(project);
        let bins = meta
            .tests()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Sp1 tests: {:?}", bins);
        inner(meta, &bins, true, "release");
    }

    pub fn inner(meta: Metadata, bins: &Vec<String>, test: bool, profile: &str) {
        rerun_if_changed(&[meta.target_directory.parent().unwrap().into()], &[]);
        let builder = GuestBuilder::new(&meta, "riscv32im-succinct-zkvm-elf", "succinct")
            .rust_flags(&[
                "passes=loweratomic",
                "link-arg=-Ttext=0x00200800",
                "panic=abort",
            ])
            .custom_args(&["--ignore-rust-version"]);
        let executor = if !test {
            builder.build_command(profile, bins)
        } else {
            builder.test_command(profile, bins)
        };
        println!("executor: {:?}", executor);

        executor
            .execute()
            .expect("Execution failed")
            .sp1_placement(&meta)
            .expect("Failed to export Sp1 artifacts");
    }
}

#[cfg(feature = "risc0")]
pub mod risc0 {
    use super::*;
    use crate::risc0_util::*;

    /// Compile the specified Ris0 binaries in the project
    pub fn bins(project: &str, bins: &[&str], dest: &[&str]) {
        let meta = parse_metadata(project);
        let bins = meta
            .bins()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Ris0 bins: {:?}", bins);
        inner(meta, &bins, dest, false, "debug");
    }

    /// Compile the specified Ris0 test in the project
    pub fn tests(project: &str, bins: &[&str], dest: &[&str]) {
        let meta = parse_metadata(project);
        let bins = meta
            .tests()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Ris0 tests: {:?}", bins);
        inner(meta, &bins, dest, true, "debug");
    }

    pub fn inner(meta: Metadata, bins: &Vec<String>, dest: &[&str], test: bool, profile: &str) {
        rerun_if_changed(&[meta.target_directory.parent().unwrap().into()], &[]);
        let mut builder =
            GuestBuilder::new(&meta, "riscv32im-risc0-zkvm-elf", "risc0").rust_flags(&[
                "passes=loweratomic",
                "link-arg=-Ttext=0x00200800",
                "link-arg=--fatal-warnings",
                "panic=abort",
            ]);
        // .cc_compiler(
        //     risc0_data()
        //         .unwrap()
        //         .join("cpp/bin/riscv32-unknown-elf-gcc"),
        // )
        // .c_flags(&["-march=rv32im", "-nostdlib"]);
        builder.unset_cargo();
        let executor = if !test {
            builder.build_command(profile, bins)
        } else {
            builder.test_command(profile, bins)
        };

        println!("executor: {:?}", executor);

        executor
            .execute()
            .expect("Execution failed")
            .risc0_placement(&meta, dest)
            .expect("Failed to export Ris0 artifacts");
    }
}

pub fn rerun_if_changed(paths: &[PathBuf], env_vars: &[&str]) {
    // Only work in build.rs
    // Tell cargo to rerun the script only if program/{src, Cargo.toml, Cargo.lock} changes
    // Ref: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html#rerun-if-changed
    for p in paths {
        println!("cargo::rerun-if-changed={}", p.display());
    }
    for v in env_vars {
        println!("cargo::rerun-if-env-changed={}", v);
    }
}
