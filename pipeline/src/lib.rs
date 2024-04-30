mod builder;
mod executor;
#[cfg(feature = "risc0")]
mod risc0_util;

use builder::{parse_metadata, GuestBuilder, GuestMetadata};
use cargo_metadata::Metadata;
use once_cell::sync::OnceCell;
use std::path::PathBuf;

static ROOT_DIR: OnceCell<PathBuf> = OnceCell::new();

#[cfg(feature = "sp1")]
pub mod sp1 {

    use super::*;

    /// Compile the specified Sp1 binaries in the project
    pub fn bins(project: &str, bins: &[&str], dest: &str) {
        ROOT_DIR.get_or_init(|| PathBuf::from(project));
        let meta = parse_metadata(project);
        let bins = meta
            .bins()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Sp1 bins: {:?}", bins);
        inner(meta, &bins, dest, false, "release");
    }

    /// Compile the specified Sp1 test in the project
    pub fn tests(project: &str, bins: &[&str], dest: &str) {
        ROOT_DIR.get_or_init(|| PathBuf::from(project));
        let meta = parse_metadata(project);
        let bins = meta
            .tests()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Sp1 tests: {:?}", bins);
        inner(meta, &bins, dest, true, "release");
    }

    pub fn inner(meta: Metadata, bins: &Vec<String>, dest: &str, test: bool, profile: &str) {
        // Only work in build.rs, when run in main has no effects
        rerun_if_changed(
            &[
                ROOT_DIR.get().unwrap().join("src"),
                ROOT_DIR.get().unwrap().join("Cargo.toml"),
                ROOT_DIR.get().unwrap().join("Cargo.lock"),
            ],
            &[],
        );
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
        println!(
            "executor: \n   ${:?}\ntargets: \n   {:?}",
            executor.cmd, executor.artifacts
        );

        executor
            .execute()
            .expect("Execution failed")
            .sp1_placement(dest)
            .expect("Failed to export Sp1 artifacts");
    }
}

#[cfg(feature = "risc0")]
pub mod risc0 {
    use super::*;

    /// Compile the specified Ris0 binaries in the project
    pub fn bins(project: &str, bins: &[&str], dest: &str) {
        ROOT_DIR.get_or_init(|| PathBuf::from(project));
        let meta = parse_metadata(project);
        let bins = meta
            .bins()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Ris0 bins: {:?}", bins);
        inner(meta, &bins, dest, false, "release");
    }

    /// Compile the specified Ris0 test in the project
    pub fn tests(project: &str, bins: &[&str], dest: &str) {
        ROOT_DIR.get_or_init(|| PathBuf::from(project));
        let meta = parse_metadata(project);
        let bins = meta
            .tests()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();

        println!("Compiling Ris0 tests: {:?}", bins);
        inner(meta, &bins, dest, true, "release");
    }

    pub fn inner(meta: Metadata, bins: &Vec<String>, dest: &str, test: bool, profile: &str) {
        // Only work in build.rs, when run in main has no effects
        rerun_if_changed(
            &[
                ROOT_DIR.get().unwrap().join("src"),
                ROOT_DIR.get().unwrap().join("Cargo.toml"),
                ROOT_DIR.get().unwrap().join("Cargo.lock"),
            ],
            &[],
        );
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
        println!(
            "executor: \n   ${:?}\ntargets: \n   {:?}",
            executor.cmd, executor.artifacts
        );
        executor
            .execute()
            .expect("Execution failed")
            .risc0_placement(dest)
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
