use anyhow::Result;
use risc0_binfmt::ProgramBinary;
use risc0_zkos_v1compat::V1COMPAT_ELF;
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    str::FromStr as _,
};

pub const DIGEST_WORDS: usize = 8;

/// Represents an item in the generated list of compiled guest binaries
#[derive(Debug, Clone)]
pub struct GuestListEntry {
    /// The name of the guest binary
    pub name: Cow<'static, str>,
    /// The compiled ELF guest binary
    #[allow(dead_code)]
    pub elf: Cow<'static, [u8]>,
    /// The image id of the guest
    pub image_id: [u32; DIGEST_WORDS],
    /// The path to the ELF binary
    pub path: Cow<'static, str>,
}

impl GuestListEntry {
    /// Builds the [GuestListEntry] by reading the user ELF from disk, combines with V1COMPAT_ELF kernel, and calculating the associated
    /// image ID.
    pub fn build(name: &str, elf_path: &str) -> Result<Self> {
        let mut elf_path = elf_path.to_owned();
        // Because the R0 build system isn't used, this does not include the kernel portion of the ELF
        let user_elf = std::fs::read(&elf_path)?;

        // Combines the user ELF with the kernel ELF
        let elf = ProgramBinary::new(&user_elf, V1COMPAT_ELF).encode();

        let image_id = risc0_binfmt::compute_image_id(&elf)?;

        let combined_path = PathBuf::from_str(&(elf_path + ".bin"))?;
        std::fs::write(&combined_path, &elf)?;
        elf_path = combined_path.to_str().unwrap().to_owned();

        println!("risc0 elf image id: {}", hex::encode(image_id.as_bytes()));
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
            fs::canonicalize(Path::new(&self.path.as_ref())).expect("Couldn't canonicalize path"),
            dest,
        )
        .map(|p| String::from(p.to_str().expect("Path is not valid UTF-8")))
        .expect("No relative path for destination");

        let upper = self.name.to_uppercase().replace('-', "_");
        let image_id: [u32; DIGEST_WORDS] = self.image_id;
        format!(
            r##"
pub const {upper}_ELF: &[u8] = include_bytes!("{relative_path}");
pub const {upper}_ID: [u32; 8] = {image_id:?};
"##
        )
    }
}
