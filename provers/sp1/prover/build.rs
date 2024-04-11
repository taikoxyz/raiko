use std::{fs, io::BufReader, path::{Path, PathBuf}, process::{Command, Stdio}, thread};
use std::io::BufRead;
use chrono::Local;
use regex::Regex;

fn main() {
    #[cfg(not(feature = "enable"))]
    println!("Sp1 not enabled");

    #[cfg(feature = "enable")]
    sp1_helper::build_program("../guest");
    #[cfg(feature = "enable")]
    build_test("../guest");
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

    execute_build_cmd(&program_dir)
        .unwrap_or_else(|_| panic!("Failed to build `{}`.", root_package_name));
}

fn current_datetime() -> String {
    let now = Local::now();
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}


/// Executes the `cargo prove build` command in the program directory
fn execute_build_cmd(
    program_dir: &Path,
) -> Result<std::process::ExitStatus, std::io::Error> {

    let mut metadata_cmd = cargo_metadata::MetadataCommand::new();
    metadata_cmd.current_dir(program_dir);
    let metadata = metadata_cmd.exec().unwrap();
    let root_package = metadata.root_package();
    let root_package_name = root_package.as_ref().map(|p| &p.name);

    
    let build_target = "riscv32im-succinct-zkvm-elf";
    let mut cmd = Command::new("cargo");
    cmd.current_dir(program_dir)
        .env("RUSTUP_TOOLCHAIN", "succinct")
        .args([
            "test",
            "--release",
            "--target",
            build_target,
            "--locked",
            "--no-run",
        ])
        .env("CARGO_MANIFEST_DIR", program_dir)
        .env_remove("RUSTC")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;

    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = BufReader::new(child.stderr.take().unwrap());
    
    
    // let stdout_lines: Vec<Result<String, _>> = stdout.lines().collect::<Vec<_>>();
    // println!("stdout_lines: {:?}", stdout_lines);
    let elf_path = stderr.lines().last().and_then(|line| {
        extract_path( &line.unwrap())
    }).expect("Failed to extract path from cargo test output");
    println!("elf_path: {:?}", elf_path);
    

    let elf_dir = metadata.target_directory.parent().unwrap().join("elf");
    let elf_path_ = metadata.target_directory.parent().unwrap().join(elf_path.to_str().unwrap());
    fs::create_dir_all(&elf_dir)?;
    println!("elf_dir: {:?}", elf_dir);
    println!("elf_path: {:?}", elf_path_);

    let result_elf_path = elf_dir.join("riscv32im-succinct-zkvm-elf-test");
    fs::copy(elf_path_, &result_elf_path)?;

    // Pipe stdout and stderr to the parent process with [sp1] prefix
    let stdout_handle = thread::spawn(move || {
        stdout.lines().for_each(|line| {
            println!("[sp1] {}", line.unwrap());
        });
    });

    // stderr.lines().for_each(|line| {
    //     eprintln!("[sp1-err] {}", line.unwrap());
    // });
    // println!("stderr last {:?}", stderr.lines().last().unwrap().unwrap());

    stdout_handle.join().unwrap();

    child.wait()
}

fn extract_path(line: &str) -> Option<PathBuf> {
    let re = Regex::new(r"\(([^)]+)\)").unwrap();
    re
        .captures(line)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .and_then(|s| Some(PathBuf::from(s)))
}