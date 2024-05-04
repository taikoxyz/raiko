mod builder;
mod executor;
#[cfg(feature = "risc0")]
mod risc0_util;

pub use builder::{parse_metadata, CommandBuilder, GuestMetadata};
pub use cargo_metadata::Metadata;
use once_cell::sync::OnceCell;
use std::path::PathBuf;

pub static ROOT_DIR: OnceCell<PathBuf> = OnceCell::new();

pub trait Pipeline {
    fn new(root: &str, profile: &str) -> Self;
    fn builder(&self) -> CommandBuilder;
    fn bins(&self, bins: &[&str], dest: &str);
    fn tests(&self, bins: &[&str], dest: &str);
}

#[cfg(feature = "risc0")]
pub mod risc0 {
    use super::*;

    pub struct Risc0Pipeline {
        pub meta: Metadata,
        pub profile: String,
    }

    impl Pipeline for Risc0Pipeline {
        fn new(root: &str, profile: &str) -> Self {
            ROOT_DIR.get_or_init(|| PathBuf::from(root));
            Risc0Pipeline {
                meta: parse_metadata(root),
                profile: profile.to_string(),
            }
        }

        fn builder(&self) -> CommandBuilder {
            let mut builder = CommandBuilder::new(&self.meta, "riscv32im-risc0-zkvm-elf", "risc0")
                .rust_flags(&[
                    "passes=loweratomic",
                    "link-arg=-Ttext=0x00200800",
                    "panic=abort",
                ])
                .cc_compiler("gcc".into())
                .c_flags(&["/opt/riscv/bin/riscv32-unknown-elf-gcc"])
                .custom_args(&["--ignore-rust-version"]);
            // Cannot use /.rustup/toolchains/risc0/bin/cargo, use regular cargo
            builder.unset_cargo();
            builder
        }

        fn bins(&self, names: &[&str], dest: &str) {
            rerun_if_changed(&[]);
            let bins = self.meta.get_bins(names);
            let builder = self.builder();
            let executor = builder.build_command(&self.profile, &bins);
            println!(
                "executor: \n   ${:?}\ntargets: \n   {:?}",
                executor.cmd, executor.artifacts
            );
            if executor.artifacts.is_empty() {
                panic!("No artifacts to build");
            }
            executor
                .execute()
                .expect("Execution failed")
                .risc0_placement(dest)
                .expect("Failed to export Sp1 artifacts");
        }

        fn tests(&self, names: &[&str], dest: &str) {
            rerun_if_changed(&[]);
            let tests = self.meta.get_tests(names);
            let builder = self.builder();
            let executor = builder.test_command(&self.profile, &tests);
            println!(
                "executor: \n   ${:?}\ntargets: \n   {:?}",
                executor.cmd, executor.artifacts
            );
            if executor.artifacts.is_empty() {
                panic!("No artifacts to build");
            }
            executor
                .execute()
                .expect("Execution failed")
                .risc0_placement(dest)
                .expect("Failed to export Sp1 artifacts");
        }
    }
}

pub fn rerun_if_changed(env_vars: &[&str]) {
    // Only work in build.rs
    // Tell cargo to rerun the script only if program/{src, Cargo.toml, Cargo.lock} changes
    // Ref: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html#rerun-if-changed
    let root = ROOT_DIR.get().unwrap();
    [
        root.join("src"),
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
    ]
    .iter()
    .for_each(|p| println!("cargo:rerun-if-changed={}", p.display()));
    for v in env_vars {
        println!("cargo::rerun-if-env-changed={}", v);
    }
}
