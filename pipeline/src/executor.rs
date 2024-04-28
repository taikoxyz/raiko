use anyhow::Result;
use cargo_metadata::Metadata;
use regex::Regex;
use std::fs::File;
use std::io::BufRead;
use std::{
    fs,
    io::{BufReader, Write},
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
            .unwrap();

        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = BufReader::new(child.stderr.take().unwrap());

        let stdout_handle = thread::spawn(move || {
            stdout.lines().for_each(|line| {
                println!("[docker] {}", line.unwrap());
            });
        });
        stderr.lines().for_each(|line| {
            let line = line.unwrap();
            println!("[zkvm-stdout] {}", line);
            if self.test && line.contains("Executable unittests") {
                if let Some(test) = extract_path(&line) {
                    self.artifacts
                        .iter_mut()
                        .find(|a| file_name(&test).contains(&file_name(a.clone())))
                        .map(|a| *a = test)
                        .expect("Failed to find test artifact");
                }
            }
        });
        stdout_handle.join().unwrap();

        let result = child.wait()?;
        if !result.success() {
            // Error message is already printed by cargo
            std::process::exit(result.code().unwrap_or(1))
        }
        Ok(self)
    }

    #[cfg(feature = "sp1")]
    pub fn sp1_placement(&self, meta: &Metadata) -> Result<()> {
        let parant = meta.target_directory.parent().unwrap();
        let dest = parant.join("elf");
        fs::create_dir_all(&dest)?;

        for src in &self.artifacts {
            let dest = dest.join(if self.test {
                format!("test-{}", file_name(src).split("-").collect::<Vec<_>>()[0])
            } else {
                file_name(src)
            });
            fs::copy(parant.join(src.to_str().unwrap()), dest.clone())?;
            println!("Copied test elf from\n[{:?}]\nto\n[{:?}]", src, dest);
        }
        Ok(())
    }

    #[cfg(feature = "risc0")]
    pub fn risc0_placement(&self, meta: &Metadata, dest: &[&str]) -> Result<()> {
        use crate::risc0_util::GuestListEntry;
        use std::env;

        assert!(dest.len() == self.artifacts.len());
        let parant = meta.target_directory.parent().unwrap();
        let _current = env::current_dir()?;
        for (i, src) in self.artifacts.iter().enumerate() {
            let mut dest = File::create(dest[i]).unwrap();
            let guest = GuestListEntry::build(
                &if self.test {
                    format!("test-{}", file_name(&src))
                } else {
                    file_name(&src)
                },
                &parant.join(src.to_str().unwrap()).to_string(),
            )
            .unwrap();
            dest.write_all(guest.codegen_consts().as_bytes())?;
            println!("Wrote from\n[{:?}]\nto\n[{:?}]", src, dest);
        }
        Ok(())
    }
}

fn file_name(path: &PathBuf) -> String {
    String::from(path.file_name().unwrap().to_str().unwrap())
}

fn extract_path(line: &str) -> Option<PathBuf> {
    let re = Regex::new(r"\(([^)]+)\)").unwrap();
    re.captures(line)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .map(PathBuf::from)
}
