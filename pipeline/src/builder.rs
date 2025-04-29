use cargo_metadata::{Metadata, Target};

use std::{collections::HashMap, env, path::PathBuf, process::Command};

use crate::{executor::Executor, ROOT_DIR};

pub fn parse_metadata(path: &str) -> Metadata {
    let manifest = std::path::Path::new(path).join("Cargo.toml");
    let mut metadata_cmd = cargo_metadata::MetadataCommand::new();

    metadata_cmd
        .no_deps()
        .manifest_path(manifest)
        .exec()
        .expect("Couldn't parse metadata")
}

pub trait GuestMetadata {
    // /// Kind of target ("bin", "example", "test", "bench", "lib", "custom-build")
    fn get_tests(&self, names: &[&str]) -> Vec<String>;

    fn get_bins(&self, names: &[&str]) -> Vec<String>;

    fn tests(&self) -> Vec<&Target>;

    fn bins(&self) -> Vec<&Target>;

    fn benchs(&self) -> Vec<&Target>;

    fn libs(&self) -> Vec<&Target>;

    fn build_scripts(&self) -> Vec<&Target>;
}

impl GuestMetadata for Metadata {
    fn get_tests(&self, names: &[&str]) -> Vec<String> {
        self.tests()
            .iter()
            .filter(|t| names.iter().any(|n| t.name.contains(n)))
            .map(|t| t.name.clone())
            .collect()
    }

    fn get_bins(&self, names: &[&str]) -> Vec<String> {
        self.bins()
            .iter()
            .filter(|t| names.iter().any(|n| t.name.contains(n)))
            .map(|t| t.name.clone())
            .collect()
    }

    fn tests(&self) -> Vec<&Target> {
        self.packages.iter().fold(Vec::new(), |mut packages, p| {
            packages.extend(p.targets.iter().filter(|t| t.test));
            packages
        })
    }

    fn bins(&self) -> Vec<&Target> {
        self.packages.iter().fold(Vec::new(), |mut packages, p| {
            packages.extend(
                p.targets
                    .iter()
                    .filter(|t| t.kind.iter().any(|k| k == "bin")),
            );
            packages
        })
    }

    fn benchs(&self) -> Vec<&Target> {
        self.packages.iter().fold(Vec::new(), |mut packages, p| {
            packages.extend(
                p.targets
                    .iter()
                    .filter(|t| t.kind.iter().any(|k| k == "bench")),
            );
            packages
        })
    }

    fn libs(&self) -> Vec<&Target> {
        self.packages.iter().fold(Vec::new(), |mut packages, p| {
            packages.extend(
                p.targets
                    .iter()
                    .filter(|t| t.kind.iter().any(|k| k == "lib")),
            );
            packages
        })
    }

    fn build_scripts(&self) -> Vec<&Target> {
        self.packages.iter().fold(Vec::new(), |mut packages, p| {
            packages.extend(
                p.targets
                    .iter()
                    .filter(|t| t.kind.iter().any(|k| k == "custom-build")),
            );
            packages
        })
    }
}

#[derive(Clone)]
pub struct CommandBuilder {
    pub meta: Metadata,

    pub target: String,

    pub sanitized_env: Vec<String>,

    pub cargo: Option<PathBuf>,
    // rustc compiler specific to toolchain
    pub rustc: Option<PathBuf>,
    // -C flags
    pub rust_flags: Option<Vec<String>>,
    // --cfg configs
    pub rust_cfgs: Option<Vec<String>>,
    // -Z flags
    pub z_flags: Option<Vec<String>>,
    // riscv32im gcc
    pub cc_compiler: Option<PathBuf>,
    // gcc flag
    pub c_flags: Option<Vec<String>>,

    pub custom_args: Vec<String>,

    custom_env: HashMap<String, String>,
}

impl CommandBuilder {
    fn get_path_buf(tool: &str, toolchain: &str) -> Option<PathBuf> {
        match sanitized_cmd("rustup")
            .args([&format!("+{toolchain}"), "which", tool])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    let stdout = output.stdout;
                    if let Ok(out) = String::from_utf8(stdout.clone()) {
                        let out = out.trim();
                        println!("Using {tool}: {out}");
                        Some(PathBuf::from(out))
                    } else {
                        println!("Command succeeded with unknown output: {stdout:?}");
                        None
                    }
                } else {
                    eprintln!("Command failed with status: {}", output.status);
                    None
                }
            }
            Err(e) => {
                eprintln!("Failed to execute command: {}", e);
                None
            }
        }
    }

    pub fn new(meta: &Metadata, target: &str, toolchain: &str) -> Self {
        Self {
            meta: meta.clone(),
            target: target.to_string(),
            cargo: CommandBuilder::get_path_buf("cargo", toolchain),
            rustc: CommandBuilder::get_path_buf("rustc", toolchain),
            sanitized_env: Vec::new(),
            rust_flags: None,
            rust_cfgs: None,
            z_flags: None,
            cc_compiler: None,
            c_flags: None,
            custom_args: Vec::new(),
            custom_env: HashMap::new(),
        }
    }

    pub fn unset_cargo(&mut self) {
        self.cargo = None;
    }

    pub fn unset_rustc(&mut self) {
        self.rustc = None;
    }

    pub fn sanitized_env(mut self, env_vars: &[&str]) -> Self {
        self.sanitized_env = to_strings(env_vars);
        self
    }

    pub fn rust_flags(mut self, flags: &[&str]) -> Self {
        self.rust_flags = Some(to_strings(flags));
        self
    }

    pub fn rust_cfgs(mut self, flags: &[&str]) -> Self {
        self.rust_cfgs = Some(to_strings(flags));
        self
    }

    pub fn z_flags(mut self, flags: &[&str]) -> Self {
        self.z_flags = Some(to_strings(flags));
        self
    }

    pub fn cc_compiler(mut self, compiler: PathBuf) -> Self {
        self.cc_compiler = Some(compiler);
        self
    }

    pub fn c_flags(mut self, flags: &[&str]) -> Self {
        self.c_flags = Some(to_strings(flags));
        self
    }

    pub fn custom_args(mut self, args: &[&str]) -> Self {
        self.custom_args = to_strings(args);
        self
    }

    pub fn custom_env(mut self, env: HashMap<String, String>) -> Self {
        self.custom_env = env;
        self
    }

    pub fn extend_custom(&self, cmd: &mut Command, args: &mut Vec<String>) {
        args.extend(self.custom_args.clone());
        for (key, val) in self.custom_env.iter() {
            cmd.env(key, val);
        }
    }

    pub fn sanitize(&self, cmd: &mut Command, filter_cargo: bool) {
        if filter_cargo {
            for (key, _val) in env::vars().filter(|(key, _)| key.starts_with("CARGO")) {
                cmd.env_remove(key);
            }
        }
        for key in self.sanitized_env.iter() {
            cmd.env_remove(key);
        }
    }

    /// Runs cargo build and returns paths of the artifacts
    /// target/
    /// ├── debug/
    ///    ├── deps/
    ///    │   |── main-<hasha>   --> this is the output
    ///    │   |── main-<hashb>
    ///    │   └── bin2-<hashe>   --> this is the output
    ///    ├── build/
    ///    ├── main               --> this is the output (same)
    ///    └── bin2               --> this is the output (same)
    pub fn build_command(&self, profile: &str, bins: &[String]) -> Executor {
        let cmd = self.inner_command(vec!["build".to_owned()], profile, bins.to_owned());

        let target_path: PathBuf = self
            .meta
            .target_directory
            .join(self.target.clone())
            .join(profile)
            .into();

        let artifacts = self
            .meta
            .bins()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| target_path.join(t.name.clone()))
            .collect::<Vec<_>>();

        Executor {
            cmd,
            artifacts,
            test: false,
        }
    }

    /// Runs cargo test and returns *incomplete* paths of the artifacts
    /// target/
    /// ├── debug/
    ///    ├── deps/
    ///    │   |── main-<hasha>
    ///    │   |── main-<hashb>    --> this is the test
    ///    │   |── bin2-<hashe>
    ///    │   └── my-test-<hashe> --> this is the test
    ///    ├── build/
    /// Thus the test artifacts path are hypothetical because we don't know the hash yet
    pub fn test_command(&self, profile: &str, bins: &Vec<String>) -> Executor {
        let cmd = self.inner_command(
            vec!["test".to_owned(), "--no-run".to_owned()],
            profile,
            bins.clone(),
        );

        let target_path: PathBuf = self
            .meta
            .target_directory
            .join(self.target.clone())
            .join(profile)
            .join("deps")
            .into();

        println!("tests {bins:?}");

        let artifacts = self
            .meta
            .tests()
            .iter()
            .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
            .map(|t| target_path.join(t.name.clone()))
            .collect::<Vec<_>>();

        Executor {
            cmd,
            artifacts,
            test: true,
        }
    }

    pub fn inner_command(
        &self,
        mut args: Vec<String>,
        profile: &str,
        mut bins: Vec<String>,
    ) -> Command {
        let CommandBuilder {
            meta,
            target,
            cargo,
            rustc,
            rust_flags,
            rust_cfgs,
            z_flags,
            cc_compiler,
            c_flags,
            ..
        } = self.clone();

        // Construct cargo args
        // `--{profile} {bin} --target {target} --locked -Z {z_flags}`
        if profile != "debug" {
            // error: unexpected argument '--debug' found; tip: `--debug` is the default
            args.push(format!("--{profile}"));
        }

        args.extend(vec![
            "--target".to_owned(),
            target,
            // "--locked".to_string(),
        ]);

        if !bins.is_empty() {
            let libs = meta
                .libs()
                .iter()
                .filter(|t| bins.iter().any(|b| b.contains(&t.name)))
                .map(|t| t.name.clone())
                .collect::<Vec<_>>();
            bins.retain(|x| !libs.contains(x));
            args.extend(format_flags("--lib", &libs));
            args.extend(format_flags("--bin", &bins));
        }

        if let Some(z_flags) = z_flags {
            args.extend(format_flags("-Z", &z_flags));
        }

        // Construct command from the toolchain-specific cargo
        let mut cmd = Command::new(cargo.map_or("cargo".to_owned(), |c| {
            c.to_str().expect("Output is not valid UTF-8").to_owned()
        }));

        // Clear unwanted env vars
        self.sanitize(&mut cmd, true);
        cmd.current_dir(ROOT_DIR.get().expect("No reference to ROOT_DIR"));

        // Set Rustc compiler path and flags
        cmd.env(
            "RUSTC",
            rustc.map_or("rustc".to_string(), |c| {
                c.to_str().expect("Output is not valid UTF-8").to_owned()
            }),
        );

        let mut encoded_flags: Vec<String> = vec![];
        if let Some(rust_flags) = rust_flags {
            encoded_flags = format_flags("-C", &rust_flags);
        }

        if let Some(cfgs) = rust_cfgs {
            encoded_flags.extend(format_flags("--cfg", &cfgs));
        }
        cmd.env("CARGO_ENCODED_RUSTFLAGS", encoded_flags.join("\x1f"));

        // Set C compiler path and flags
        if let Some(cc_compiler) = cc_compiler {
            cmd.env("CC", cc_compiler);
        }

        if let Some(c_flags) = c_flags {
            cmd.env(format!("CC_{}", self.target), c_flags.join(" "));
        }

        self.extend_custom(&mut cmd, &mut args);
        cmd.args(args);

        cmd
    }
}

fn to_strings(strs: &[&str]) -> Vec<String> {
    strs.iter().map(|s| s.to_string()).collect()
}

pub fn format_flags(flag: &str, items: &[String]) -> Vec<String> {
    items.iter().fold(Vec::new(), |mut res, i| {
        res.extend([flag.to_owned(), i.to_owned()]);
        res
    })
}

fn sanitized_cmd(tool: &str) -> Command {
    let mut cmd = Command::new(tool);
    for (key, _val) in env::vars().filter(|(key, _)| key.starts_with("CARGO")) {
        cmd.env_remove(key);
    }
    cmd.env_remove("RUSTUP_TOOLCHAIN");
    cmd
}
