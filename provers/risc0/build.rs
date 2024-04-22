fn main() {
    #[cfg(not(feature = "enable"))]
    println!("Risc0 not enabled");

    #[cfg(feature = "enable")]
    risc0_build::embed_methods();
}

fn embed_tests() {
    let out_dir_env = env::var_os("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir_env); // $ROOT/target/$profile/build/$crate/out
    let guest_dir = get_guest_dir();

    // Read the cargo metadata for info from `[package.metadata.risc0]`.
    let pkg = current_package();
    let guest_packages = guest_packages(&pkg);
    let methods_path = out_dir.join("test.rs");
    let mut methods_file = File::create(&methods_path).unwrap();

    detect_toolchain(RUSTUP_TOOLCHAIN_NAME);

    build_guest_package(&guest_pkg, &guest_dir, &guest_opts, None);
    let methods = guest_methods(&guest_pkg, &guest_dir)
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
                    .context("elf path contains invalid unicode")
                    .unwrap(),
            )
            .unwrap()
        })
        .collect()
}


// Builds a package that targets the riscv guest into the specified target
// directory.
fn build_guest_package<P>(
    pkg: &Package,
    target_dir: P,
    guest_opts: &GuestOptions,
    runtime_lib: Option<&str>,
) where
    P: AsRef<Path>,
{
    if !get_env_var("RISC0_SKIP_BUILD").is_empty() {
        return;
    }

    fs::create_dir_all(target_dir.as_ref()).unwrap();

    let mut cmd = if let Some(lib) = runtime_lib {
        cargo_command("test", &["-C", &format!("link_arg={}", lib)])
    } else {
        cargo_command("test", &[])
    };
    cmd.args(["--no-run"]);

    let features_str = guest_opts.features.join(",");
    if !features_str.is_empty() {
        cmd.args(["--features", &features_str]);
    }

    cmd.args([
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
