use std::{
    borrow::Cow,
    collections::HashMap,
    default::Default,
    env,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use cargo_metadata::{Message, MetadataCommand, Package};
use serde::Deserialize;

pub const DIGEST_WORDS: usize = 8;

#[derive(Debug, Deserialize)]
pub struct Risc0Metadata {
    pub methods: Vec<String>,
}

impl Risc0Metadata {
    pub fn from_package(pkg: &Package) -> Option<Risc0Metadata> {
        let obj = pkg.metadata.get("risc0").unwrap();
        serde_json::from_value(obj.clone()).unwrap()
    }
}

/// Represents an item in the generated list of compiled guest binaries
#[derive(Debug, Clone)]
pub struct GuestListEntry {
    /// The name of the guest binary
    pub name: Cow<'static, str>,
    /// The compiled ELF guest binary
    pub elf: Cow<'static, [u8]>,
    /// The image id of the guest
    pub image_id: [u32; DIGEST_WORDS],
    /// The path to the ELF binary
    pub path: Cow<'static, str>,
}

impl GuestListEntry {
    /// Builds the [GuestListEntry] by reading the ELF from disk, and calculating the associated
    /// image ID.
    pub fn build(name: &str, elf_path: &str) -> Result<Self> {
        println!("*************  {} *************", elf_path);
        let elf = std::fs::read(elf_path)?;
        println!("******--*****  {} ******--*****", elf_path);

        // Todo(Cecilia)
        let image_id = [9u32; DIGEST_WORDS];

        Ok(Self {
            name: Cow::Owned(name.to_owned()),
            elf: Cow::Owned(elf),
            image_id,
            path: Cow::Owned(elf_path.to_owned()),
        })
    }

    pub fn codegen_consts(&self) -> String {
        // Quick check for '#' to avoid injection of arbitrary Rust code into the the
        // method.rs file. This would not be a serious issue since it would only
        // affect the user that set the path, but it's good to add a check.
        if self.path.contains('#') {
            panic!("method path cannot include #: {}", self.path);
        }

        let upper = self.name.to_uppercase().replace('-', "_");
        let mut parts: Vec<&str> = upper.split('_').collect();
        parts.pop();
        parts.push("TEST");
        let upper = parts.join("_");

        let image_id: [u32; DIGEST_WORDS] = self.image_id;
        let elf_path: &str = &self.path;
        let elf_contents: &[u8] = &self.elf;
        let f = format!(
            r##"
pub const {upper}_ELF: &[u8] = &{elf_contents:?};
pub const {upper}_ID: [u32; 8] = {image_id:?};
pub const {upper}_PATH: &str = r#"{elf_path}"#;
"##
        );
        println!(
            "elf_contents: {:?}",
            [
                elf_contents[100],
                elf_contents[101],
                elf_contents[102],
                elf_contents[103],
                elf_contents[104],
                elf_contents[105],
                elf_contents[106],
                elf_contents[107]
            ]
        );
        println!("elf_path: {:?}", elf_path);
        f
    }
}

pub fn is_debug() -> bool {
    get_env_var("RISC0_BUILD_DEBUG") == "1"
}

pub fn get_env_var(name: &str) -> String {
    println!("cargo:rerun-if-env-changed={name}");
    env::var(name).unwrap_or_default()
}
