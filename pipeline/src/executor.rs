use anyhow::bail;
use regex::Regex;
use std::io::BufRead;
use std::path::Path;
use std::{
    io::BufReader,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
};

#[derive(Debug)]
pub struct Executor {
    pub cmd: Command,
    pub artifacts: Vec<PathBuf>,
    pub test: bool,
}

impl Executor {
    pub fn execute(mut self) -> anyhow::Result<Self> {
        let mut child = self
            .cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Couldn't spawn child process");

        let stdout = BufReader::new(child.stdout.take().expect("Couldn't take stdout of child"));
        let stderr = BufReader::new(child.stderr.take().expect("Couldn't take stderr of child"));

        let stdout_handle = thread::spawn(move || {
            for line in stdout.lines().enumerate().map(|(index, line)| {
                line.unwrap_or_else(|e| {
                    panic!("Couldn't get stdout line: {index}\n with error: {e}")
                })
            }) {
                println!("[docker] {line}");
            }
        });

        for line in stderr.lines().enumerate().map(|(index, line)| {
            line.unwrap_or_else(|e| panic!("Couldn't get stderr line: {index}\n with error: {e}"))
        }) {
            println!("[zkvm-stdout] {line}");

            if self.test && line.contains("Executable unittests") {
                if let Some(test) = extract_path(&line) {
                    let Some(artifact) = self
                        .artifacts
                        .iter_mut()
                        .find(|a| file_name(&test).contains(&file_name(a).replace('-', "_")))
                    else {
                        bail!("Failed to find test artifact");
                    };

                    *artifact = test;
                }
            }
        }

        stdout_handle
            .join()
            .expect("Couldn't wait for stdout handle to finish");

        let result = child.wait()?;
        if !result.success() {
            // Error message is already printed by cargo
            std::process::exit(result.code().unwrap_or(1))
        }
        Ok(self)
    }

    #[cfg(feature = "sp1")]
    pub fn sp1_placement(&self, dest: &str) -> anyhow::Result<()> {
        use sp1_sdk::{CpuProver, HashableKey, Prover};
        use std::fs;

        let root = crate::ROOT_DIR.get().expect("No reference to ROOT_DIR");
        let dest = PathBuf::from(dest);

        if !dest.exists() {
            fs::create_dir_all(&dest).expect("Couldn't create destination directories");
        }

        for src in &self.artifacts {
            let mut name = file_name(src);
            if self.test {
                name = format!(
                    "test-{}",
                    name.split('-').next().expect("Couldn't get test name")
                );
            }

            fs::copy(
                root.join(src.to_str().expect("File name is not valid UTF-8")),
                &dest.join(&name.replace('_', "-")),
            )?;

            println!("Write elf from\n {src:?}\nto\n {dest:?}");
            let elf = std::fs::read(&dest.join(&name.replace('_', "-")))?;
            let prover = CpuProver::new();
            let key_pair = prover.setup(&elf);
            println!("sp1 elf vk bn256 is: {}", key_pair.1.bytes32());
            println!(
                "sp1 elf vk hash_bytes is: {}",
                hex::encode(key_pair.1.hash_bytes())
            );
        }

        Ok(())
    }

    #[cfg(feature = "risc0")]
    pub fn risc0_placement(&self, dest: &str) -> anyhow::Result<()> {
        use crate::risc0_util::GuestListEntry;
        use std::{fs, io::Write};

        let root = crate::ROOT_DIR.get().expect("No reference to ROOT_DIR");
        let dest_dir = PathBuf::from(dest);
        if !dest_dir.exists() {
            fs::create_dir_all(&dest_dir).expect("Couldn't create destination directories");
        }

        for src in &self.artifacts {
            let mut name = file_name(src);

            if self.test {
                name = format!(
                    "test-{}",
                    name.split('-').next().expect("Couldn't get test name")
                );
            }

            let mut dest_file =
                fs::File::create(&dest_dir.join(&format!("{}.rs", name.replace('-', "_"))))
                    .expect("Couldn't create destination file");

            let guest = GuestListEntry::build(
                &name,
                root.join(src).to_str().expect("Path is not valid UTF-8"),
            )
            .expect("Couldn't build the guest list entry");

            dest_file.write_all(
                guest
                    .codegen_consts(
                        &std::fs::canonicalize(&dest_dir)
                            .expect("Couldn't canonicalize the destination path"),
                    )
                    .as_bytes(),
            )?;

            println!("Write from\n {src:?}\nto\n {dest_file:?}");
        }

        Ok(())
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .expect("no filename in path")
        .to_str()
        .expect("filename is non unicode")
        .to_owned()
}

fn extract_path(line: &str) -> Option<PathBuf> {
    let re = Regex::new(r"\(([^)]+)\)").expect("Couldn't create regex");
    re.captures(line)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .map(PathBuf::from)
}
