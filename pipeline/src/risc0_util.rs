use anyhow::Result;
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

pub const DIGEST_WORDS: usize = 8;

pub fn risc0_data() -> Result<PathBuf> {
    let dir = if let Ok(dir) = std::env::var("RISC0_DATA_DIR") {
        dir.into()
    } else if let Some(root) = dirs::data_dir() {
        root.join("cargo-risczero")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".cargo-risczero")
    } else {
        anyhow::bail!("Could not determine cargo-risczero data dir. Set RISC0_DATA_DIR env var.");
    };

    Ok(dir)
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
        let elf = std::fs::read(elf_path)?;
        let image_id = risc0_binfmt::compute_image_id(&elf)?;

        Ok(Self {
            name: Cow::Owned(name.to_owned()),
            elf: Cow::Owned(elf),
            image_id: image_id.into(),
            path: Cow::Owned(elf_path.to_owned()),
        })
    }

    pub fn codegen_consts(&self, dest: &PathBuf) -> String {
        if self.path.contains('#') {
            panic!("method path cannot include #: {}", self.path);
        }
        let relative_path = pathdiff::diff_paths(
            fs::canonicalize(Path::new(&self.path.as_ref())).unwrap(),
            dest,
        )
        .map(|p| String::from(p.to_str().unwrap()))
        .unwrap();

        let upper = self.name.to_uppercase().replace('-', "_");
        let image_id: [u32; DIGEST_WORDS] = self.image_id;
        let elf_path: &str = &self.path;
        format!(
            r##"
pub const {upper}_ELF: &[u8] = include_bytes!("{relative_path}");
pub const {upper}_ID: [u32; 8] = {image_id:?};
pub const {upper}_PATH: &str = r#"{elf_path}"#;
"##
        )
    }
}
