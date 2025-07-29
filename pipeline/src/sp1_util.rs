use anyhow::Result;
use sp1_sdk::{CpuProver, HashableKey, Prover};
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

/// Represents an item in the generated list of compiled SP1 guest binaries
#[derive(Debug, Clone)]
pub struct Sp1GuestListEntry {
    /// The name of the guest binary
    pub name: Cow<'static, str>,
    /// The compiled ELF guest binary
    #[allow(dead_code)]
    pub elf: Cow<'static, [u8]>,
    /// The vk bn256 bytes32 representation
    pub vk_bn256: String,
    /// The vk hash_bytes representation
    pub vk_hash_bytes: String,
    /// The path to the ELF binary
    pub path: Cow<'static, str>,
}

impl Sp1GuestListEntry {
    /// Builds the [Sp1GuestListEntry] by reading the ELF from disk and calculating the associated
    /// verification key values.
    pub fn build(name: &str, elf_path: &str) -> Result<Self> {
        let elf_path = elf_path.to_owned();
        let elf = std::fs::read(&elf_path)?;

        let prover = CpuProver::new();
        let key_pair = prover.setup(&elf);
        let vk_bn256 = key_pair.1.bytes32();
        let vk_hash_bytes = hex::encode(key_pair.1.hash_bytes());

        Ok(Self {
            name: Cow::Owned(name.to_owned()),
            elf: Cow::Owned(elf),
            vk_bn256,
            vk_hash_bytes,
            path: Cow::Owned(elf_path),
        })
    }

    pub fn codegen_consts(&self, dest: &PathBuf) -> String {
        if self.path.contains('#') {
            panic!("method path cannot include #: {}", self.path);
        }
        let relative_path = pathdiff::diff_paths(
            fs::canonicalize(Path::new(&self.path.as_ref())).expect("Couldn't canonicalize path"),
            dest,
        )
        .map(|p| String::from(p.to_str().expect("Path is not valid UTF-8")))
        .expect("No relative path for destination");

        let upper = self.name.to_uppercase().replace('-', "_");
        format!(
            r##"pub const {upper}_ELF: &[u8] = include_bytes!("{relative_path}");
pub const {upper}_VK_BN256: &str = "{}";
pub const {upper}_VK_HASH_BYTES: &str = "{}";
"##,
            self.vk_bn256, self.vk_hash_bytes
        )
    }
}
