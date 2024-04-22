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

use cargo_metadata::{Message, MetadataCommand, Package};

mod utils;
use risc0_build::cargo_command;
use utils::*;

fn main() {
    #[cfg(not(feature = "enable"))]
    println!("Risc0 not enabled");

    #[cfg(feature = "enable")]
    risc0_build::embed_methods();

    #[cfg(all(feature = "enable"))]
    embed_tests();
}

fn embed_tests() -> Vec<GuestListEntry> {
    let out_dir_env = env::var_os("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir_env); // $ROOT/target/$profile/build/$crate/out
                                           // Determine the output directory, in the target folder, for the guest binary.
    let guest_dir = out_dir
        .parent() // out
        .unwrap()
        .parent() // $crate
        .unwrap()
        .parent() // build
        .unwrap()
        .parent() // $profile
        .unwrap()
        .join("riscv-guest");

    // Read the cargo metadata for info from `[package.metadata.risc0]`.
    let pkg = get_package(env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest_dir = pkg.manifest_path.parent().unwrap();
    // methods = ["guest"]
    let guest_packages = guest_packages(&pkg);

    let methods_path = out_dir.join("test.rs");
    let mut methods_file = File::create(&methods_path).unwrap();

    let mut guest_list = vec![];
    for guest_pkg in guest_packages {
        println!("Building guest package {}.{}", pkg.name, guest_pkg.name);

        build_guest_package(&guest_pkg, &guest_dir, None);
        let methods = guest_methods(&guest_pkg, &guest_dir);

        for method in methods {
            methods_file
                .write_all(method.codegen_consts().as_bytes())
                .unwrap();
            guest_list.push(method);
        }
    }
    println!("cargo:rerun-if-changed={}", methods_path.display());
    guest_list
}

/// Returns all inner packages specified the "methods" list inside
/// "package.metadata.risc0".
fn guest_packages(pkg: &Package) -> Vec<Package> {
    let manifest_dir = pkg.manifest_path.parent().unwrap();

    Risc0Metadata::from_package(pkg)
        .unwrap()
        .methods
        .iter()
        .map(|inner| get_package(manifest_dir.join(inner)))
        .collect()
}

/// Returns the given cargo Package from the metadata in the Cargo.toml manifest
/// within the provided `manifest_dir`.
pub fn get_package(manifest_dir: impl AsRef<Path>) -> Package {
    let manifest_path = manifest_dir.as_ref().join("Cargo.toml");
    let manifest_meta = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .no_deps()
        .exec()
        .expect("cargo metadata command failed");
    let mut matching: Vec<Package> = manifest_meta
        .packages
        .into_iter()
        .filter(|pkg| {
            let std_path: &Path = pkg.manifest_path.as_ref();
            std_path == manifest_path
        })
        .collect();
    if matching.is_empty() {
        eprintln!(
            "ERROR: No package found in {}",
            manifest_dir.as_ref().display()
        );
        std::process::exit(-1);
    }
    if matching.len() > 1 {
        eprintln!(
            "ERROR: Multiple packages found in {}",
            manifest_dir.as_ref().display()
        );
        std::process::exit(-1);
    }
    matching.pop().unwrap()
}

/// Returns all methods associated with the given guest crate.
fn guest_methods(pkg: &Package, target_dir: impl AsRef<Path>) -> Vec<GuestListEntry> {
    let profile = if is_debug() { "debug" } else { "release" };
    pkg.targets
        .iter()
        .filter(|target| target.kind.iter().any(|kind| kind == "test"))
        .map(|target| {
            GuestListEntry::build(
                &target.name,
                target_dir
                    .as_ref()
                    .join("riscv32im-risc0-zkvm-elf")
                    .join(profile)
                    .join(&target.name)
                    .to_str()
                    .unwrap(),
            )
            .unwrap()
        })
        .collect()
}

// Builds a package that targets the riscv guest into the specified target
// directory.
fn build_guest_package<P>(pkg: &Package, target_dir: P, runtime_lib: Option<&str>)
where
    P: AsRef<Path>,
{
    fs::create_dir_all(target_dir.as_ref()).unwrap();

    let mut cmd = cargo_command("test", &[]);
    cmd.args([
        "--no-run",
        "--manifest-path",
        pkg.manifest_path.as_str(),
        "--target-dir",
        target_dir.as_ref().to_str().unwrap(),
    ]);

    if !is_debug() {
        cmd.args(["--release"]);
    }

    let mut child = cmd
        .stderr(Stdio::piped())
        .spawn()
        .expect("cargo build failed");
    let stderr = child.stderr.take().unwrap();

    // HACK: Attempt to bypass the parent cargo output capture and
    // send directly to the tty, if available.  This way we get
    // progress messages from the inner cargo so the user doesn't
    // think it's just hanging.
    let tty_file = env::var("RISC0_GUEST_LOGFILE").unwrap_or_else(|_| "/dev/tty".to_string());

    let mut tty = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(tty_file)
        .ok();

    if let Some(tty) = &mut tty {
        writeln!(
            tty,
            "{}: Starting build for riscv32im-risc0-zkvm-elf",
            pkg.name
        )
        .unwrap();
    }

    for line in BufReader::new(stderr).lines() {
        match &mut tty {
            Some(tty) => writeln!(tty, "{}: {}", pkg.name, line.unwrap()).unwrap(),
            None => eprintln!("{}", line.unwrap()),
        }
    }

    let res = child.wait().expect("Guest 'cargo build' failed");
    if !res.success() {
        std::process::exit(res.code().unwrap());
    }
}
