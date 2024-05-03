use anyhow::Result;

use crate::ROOT_DIR;
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
                        .find(|a| file_name(&test).contains(&file_name(a).replace('-', "_")))
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
    pub fn sp1_placement(&self, dest: &str) -> Result<()> {
        let root = ROOT_DIR.get().unwrap();
        let dest = PathBuf::from(dest);
        if !dest.exists() {
            fs::create_dir_all(&dest).unwrap();
        }
        for src in &self.artifacts {
            let mut name = file_name(src);
            if self.test {
                name = format!("test-{}", name.split('-').collect::<Vec<_>>()[0]);
            }
            fs::copy(root.join(src.to_str().unwrap()), &dest.join(&name.replace('_', "-")))?;
            println!("Write elf from\n    {:?}\nto\n    {:?}", src, dest);
        }
        Ok(())
    }

    #[cfg(feature = "risc0")]
    pub fn risc0_placement(&self, dest: &str) -> Result<()> {
        use crate::risc0_util::GuestListEntry;
        let root = ROOT_DIR.get().unwrap();
        let dest = PathBuf::from(dest);
        if !dest.exists() {
            fs::create_dir_all(&dest).unwrap();
        }
        for src in &self.artifacts {
            let mut name = file_name(src);
            if self.test {
                name = format!("test-{}", name.split('-').collect::<Vec<_>>()[0]).to_string();
            }
            let mut dest =
                File::create(dest.join(&format!("{}.rs", name.replace('-', "_")))).unwrap();
            let guest = GuestListEntry::build(&name, root.join(src).to_str().unwrap()).unwrap();
            dest.write_all(guest.codegen_consts().as_bytes())?;
            println!("Write from\n  {:?}\nto\n  {:?}", src, dest);
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
