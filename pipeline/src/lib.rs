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

pub fn rerun_if_changed(env_vars: &[&str]) {
    // Only work in build.rs
    // Tell cargo to rerun the script only if program/{src, Cargo.toml, Cargo.lock} changes
    // Ref: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html#rerun-if-changed
    let root = ROOT_DIR.get().expect("No reference to ROOT_DIR");
    for p in [
        root.join("src"),
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
    ] {
        println!("cargo::rerun-if-changed={}", p.display());
    }
    for v in env_vars {
        println!("cargo::rerun-if-env-changed={}", v);
    }
}
