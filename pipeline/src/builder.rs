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
        .unwrap()
}

pub trait GuestMetadata {
    // /// Kind of target ("bin", "example", "test", "bench", "lib", "custom-build")
    fn tests(&self) -> Vec<&Target>;
    fn bins(&self) -> Vec<&Target>;
    fn examples(&self) -> Vec<&Target>;
    fn benchs(&self) -> Vec<&Target>;
    fn libs(&self) -> Vec<&Target>;
    fn build_scripts(&self) -> Vec<&Target>;
}

impl GuestMetadata for Metadata {
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

    fn examples(&self) -> Vec<&Target> {
        self.packages.iter().fold(Vec::new(), |mut packages, p| {
            packages.extend(
                p.targets
                    .iter()
                    .filter(|t| t.kind.iter().any(|k| k == "example")),
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
pub struct GuestBuilder {
    pub meta: Metadata,

    pub target: String,

    pub sanitized_env: Vec<String>,

    pub cargo: Option<PathBuf>,

    // rustc compiler specific to toolchain
    pub rustc: Option<PathBuf>,
    // -C flags
    pub rust_flags: Option<Vec<String>>,
    // -Z flags
    pub z_flags: Option<Vec<String>>,
    // riscv32im gcc
    pub cc_compiler: Option<PathBuf>,
    // gcc flag
    pub c_flags: Option<Vec<String>>,

    pub custom_args: Vec<String>,

    custom_env: HashMap<String, String>,
}

impl GuestBuilder {
    pub fn new(meta: &Metadata, target: &str, toolchain: &str) -> Self {
        let tools = ["cargo", "rustc"]
            .into_iter()
            .map(|tool| {
                let out = sanitized_cmd("rustup")
                    .args([format!("+{toolchain}").as_str(), "which", tool])
                    .output()
                    .expect("rustup failed to find {toolchain} toolchain")
                    .stdout;
                let out = String::from_utf8(out).unwrap();
                let out = out.trim();
                println!("Using rustc: {out}");
                PathBuf::from(out)
            })
            .collect::<Vec<_>>();
        Self {
            meta: meta.clone(),
            target: target.to_string(),
            sanitized_env: Vec::new(),
            cargo: Some(tools[0].clone()),
            rustc: Some(tools[1].clone()),
            rust_flags: None,
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
            for (key, _val) in env::vars().filter(|x| x.0.starts_with("CARGO")) {
                cmd.env_remove(key);
            }
        }
        self.sanitized_env.iter().for_each(|e| {
            cmd.env_remove(e);
        });
    }

    /// Runs cargo build and returns paths of the artifacts
    // target/
    // ├── debug/
    //    ├── deps/
    //    │   |── main-<hasha>   --> this is the output
    //    │   |── main-<hashb>
    //    │   └── bin2-<hashe>   --> this is the output
    //    ├── build/
    //    ├── main               --> this is the output (same)
    //    └── bin2               --> this is the output (same)
    pub fn build_command(&self, profile: &str, bins: &Vec<String>) -> Executor {
        let args = vec!["build".to_string()];
        let cmd = self.inner_command(args, profile, bins.clone());
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
    // target/
    // ├── debug/
    //    ├── deps/
    //    │   |── main-<hasha>
    //    │   |── main-<hashb>    --> this is the test
    //    │   |── bin2-<hashe>
    //    │   └── my-test-<hashe> --> this is the test
    //    ├── build/
    // Thus the test artifacts path are hypothetical because we don't know the hash yet
    pub fn test_command(&self, profile: &str, bins: &Vec<String>) -> Executor {
        let args = vec!["test".to_string(), "--no-run".to_string()];
        let cmd = self.inner_command(args, profile, bins.clone());
        let target_path: PathBuf = self
            .meta
            .target_directory
            .join(self.target.clone())
            .join(profile)
            .join("deps")
            .into();
        println!("tests {:?}", bins);
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
        let GuestBuilder {
            meta,
            target,
            cargo,
            rustc,
            rust_flags,
            z_flags,
            cc_compiler,
            c_flags,
            ..
        } = self.clone();

        assert_eq!(1, 2);

        // Construct cargo args
        // `--{profile} {bin} --target {target} --locked -Z {z_flags}`
        if profile != "debug" {
            // error: unexpected argument '--debug' found; tip: `--debug` is the default
            args.push(format!("--{}", profile));
        }
        args.extend(vec![
            "--target".to_string(),
            target.clone(),
            "--locked".to_string(),
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
        let mut cmd =
            Command::new(cargo.map_or("cargo".to_string(), |c| String::from(c.to_str().unwrap())));
        // Clear unwanted env vars
        self.sanitize(&mut cmd, true);
        cmd.current_dir(ROOT_DIR.get().unwrap());

        // Set Rustc compiler path and flags
        cmd.env(
            "RUSTC",
            rustc.map_or("rustc".to_string(), |c| String::from(c.to_str().unwrap())),
        );
        if let Some(rust_flags) = rust_flags {
            cmd.env(
                "CARGO_ENCODED_RUSTFLAGS",
                format_flags("-C", &rust_flags).join("\x1f"),
            );
        }

        // Set C compiler path and flags
        if let Some(cc_compiler) = cc_compiler {
            cmd.env("CC", cc_compiler);
        }
        if let Some(c_flags) = c_flags {
            cmd.env(format!("CFLAGS_{}", self.target), c_flags.join(" "));
        }

        self.extend_custom(&mut cmd, &mut args);
        cmd.args(args);

        cmd
    }
}

fn to_strings(strs: &[&str]) -> Vec<String> {
    println!("{:?}", strs);
    let r = strs.iter().map(|s| s.to_string()).collect();
    println!("{:?}", r);
    r
}

pub fn format_flags(flag: &str, items: &Vec<String>) -> Vec<String> {
    let res = items.iter().fold(Vec::new(), |mut res, i| {
        res.extend([flag.to_owned(), i.to_owned()]);
        res
    });
    res
}

fn sanitized_cmd(tool: &str) -> Command {
    let mut cmd = Command::new(tool);
    for (key, _val) in env::vars().filter(|x| x.0.starts_with("CARGO")) {
        cmd.env_remove(key);
    }
    cmd.env_remove("RUSTUP_TOOLCHAIN");
    cmd
}
