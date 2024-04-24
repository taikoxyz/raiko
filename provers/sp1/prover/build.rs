use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use chrono::Local;
use regex::Regex;

fn main() {
    #[cfg(not(feature = "enable"))]
    println!("Sp1 not enabled");

    // #[cfg(feature = "enable")]
    // sp1_helper::build_program("../testt");

    // #[cfg(all(feature = "enable", test))]
    #[cfg(feature = "enable")]
    build_test("../testt");
}

pub fn build_test(path: &str) {
    let program_dir = std::path::Path::new(path);

    // Tell cargo to rerun the script only if program/{src, Cargo.toml, Cargo.lock} changes
    // Ref: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html#rerun-if-changed
    let dirs = vec![
        program_dir.join("src"),
        program_dir.join("Cargo.toml"),
        program_dir.join("Cargo.lock"),
    ];
    for dir in dirs {
        println!("cargo:rerun-if-changed={}", dir.display());
    }

    // Print a message so the user knows that their program was built. Cargo caches warnings emitted
    // from build scripts, so we'll print the date/time when the program was built.
    let metadata_file = program_dir.join("Cargo.toml");
    let mut metadata_cmd = cargo_metadata::MetadataCommand::new();
    let metadata = metadata_cmd.manifest_path(metadata_file).exec().unwrap();
    let root_package = metadata.root_package();
    let root_package_name = root_package
        .as_ref()
        .map(|p| p.name.as_str())
        .unwrap_or("Program");
    println!(
        "cargo:warning={} built at {}",
        root_package_name,
        current_datetime()
    );

    execute_build_cmd(program_dir)
        .unwrap_or_else(|_| panic!("Failed to build `{}`.", root_package_name));
}

fn current_datetime() -> String {
    let now = Local::now();
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Executes the `cargo prove build` command in the program directory
fn execute_build_cmd(program_dir: &Path) -> Result<std::process::ExitStatus, std::io::Error> {
    let mut metadata_cmd = cargo_metadata::MetadataCommand::new();
    metadata_cmd.current_dir(program_dir);
    let metadata = metadata_cmd.exec().unwrap();
    let root_package = metadata.root_package();
    let root_package_name = root_package.as_ref().map(|p| &p.name);

    // println!("metadata: {:?}", metadata);
    println!("root_package: {:?}", root_package_name);

    let build_target = "riscv32im-succinct-zkvm-elf";
    let rust_flags = [
        "-C",
        "passes=loweratomic",
        "-C",
        "link-arg=-Ttext=0x00200800",
        "-C",
        "panic=abort",
    ];

    let mut cmd = Command::new("cargo");
    cmd.current_dir(program_dir)
        .env("RUSTUP_TOOLCHAIN", "succinct")
        .env("CARGO_MANIFEST_DIR", program_dir)
        .env("CARGO_ENCODED_RUSTFLAGS", rust_flags.join("\x1f"))
        .args([
            "test",
            "--release",
            "--target",
            build_target,
            "--locked",
            "--no-run",
        ])
        .env_remove("RUSTC")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;

    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = BufReader::new(child.stderr.take().unwrap());

    let elf_paths = stderr
        .lines()
        .filter(|line| {
            println!("line: {:?}", line.as_ref().unwrap());
            line.as_ref()
                .is_ok_and(|l| l.contains("Executable unittests"))
        })
        .map(|line| extract_path(&line.unwrap()).unwrap())
        .collect::<Vec<_>>();
    println!("elf_paths: {:?}", elf_paths);

    let src_elf_path = metadata.target_directory.parent().unwrap().join(
        elf_paths
            .first()
            .expect("Failed to extract carge test elf path")
            .to_str()
            .unwrap(),
    );
    println!("src_elf_path: {:?}", src_elf_path);

    let mut dest_elf_path = metadata.target_directory.parent().unwrap().join("elf");
    fs::create_dir_all(&dest_elf_path)?;
    dest_elf_path = dest_elf_path.join("riscv32im-succinct-zkvm-elf-test");
    println!("dest_elf_path: {:?}", dest_elf_path);

    fs::copy(&src_elf_path, &dest_elf_path)?;
    println!(
        "Copied test elf from\n[{:?}]\nto\n[{:?}]",
        src_elf_path, dest_elf_path
    );

    // Pipe stdout and stderr to the parent process with [sp1] prefix
    let stdout_handle = thread::spawn(move || {
        stdout.lines().for_each(|line| {
            println!("[sp1] {}", line.unwrap());
        });
    });

    stdout_handle.join().unwrap();

    child.wait()
}

fn extract_path(line: &str) -> Option<PathBuf> {
    let re = Regex::new(r"\(([^)]+)\)").unwrap();
    re.captures(line)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .map(PathBuf::from)
}
