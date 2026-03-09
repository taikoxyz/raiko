use raiko_pipeline::{
    parse_metadata, rerun_if_changed, CommandBuilder, GuestMetadata, Metadata, Pipeline,
};
use std::{env, path::PathBuf};

/// Search `PATH` for an executable by name and return the full path if found.
fn find_on_path(bin: &str) -> Option<String> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(bin);
            full.is_file().then(|| full.to_string_lossy().to_string())
        })
    })
}

fn main() {
    let pipeline = Sp1Pipeline::new("provers/sp1/guest", "release");
    pipeline.bins(
        &["sp1-aggregation", "sp1-batch", "sp1-shasta-aggregation"],
        "provers/sp1/guest/elf",
    );
    #[cfg(feature = "test")]
    pipeline.tests(&["sp1-batch"], "provers/sp1/guest/elf");
}

pub struct Sp1Pipeline {
    pub meta: Metadata,
    pub profile: String,
}

impl Pipeline for Sp1Pipeline {
    fn new(root: &str, profile: &str) -> Self {
        raiko_pipeline::ROOT_DIR.get_or_init(|| PathBuf::from(root));
        Sp1Pipeline {
            meta: parse_metadata(root),
            profile: profile.to_string(),
        }
    }

    fn builder(&self) -> CommandBuilder {
        let home_dir = env::var("HOME").unwrap_or_else(|_| {
            env::var("USERPROFILE").expect("Neither HOME nor USERPROFILE is set")
        });
        let sp1_dir = PathBuf::from(&home_dir).join(".sp1");
        let sp1_gcc = sp1_dir.join("bin/riscv64-unknown-elf-gcc");
        let builder = CommandBuilder::new(&self.meta, "riscv64im-succinct-zkvm-elf", "succinct")
            .rust_flags(&[
                "passes=lower-atomic",
                "link-arg=--image-base=0x78000000",
                "panic=abort",
                "llvm-args=-misched-prera-direction=bottomup",
                "llvm-args=-misched-postra-direction=bottomup",
            ])
            .rust_cfgs(&["getrandom_backend=\"custom\""])
            .custom_args(&["--ignore-rust-version"]);

        // Resolve riscv64 C cross-compiler.
        // 1. SP1 toolchain GCC at ~/.sp1/bin/riscv64-unknown-elf-gcc
        // 2. System riscv64-elf-gcc (e.g. from apt or Homebrew)
        // 3. System riscv64-unknown-elf-gcc
        let gcc_path = [
            Some(sp1_gcc.to_string_lossy().to_string()),
            find_on_path("riscv64-elf-gcc"),
            find_on_path("riscv64-unknown-elf-gcc"),
        ]
        .into_iter()
        .flatten()
        .find(|p| std::path::Path::new(p).exists());

        // Discover SP1 newlib sysroot headers (platform-independent lookup).
        // The riscv C toolchain lives under ~/.sp1/riscv/<platform-triple>/riscv32-unknown-elf/include
        let sp1_include_dir = sp1_dir.join("riscv").read_dir().ok().and_then(|mut rd| {
            rd.find_map(|e| {
                let dir = e.ok()?.path();
                let inc = dir.join("riscv32-unknown-elf/include");
                inc.is_dir().then(|| inc.to_string_lossy().to_string())
            })
        });

        if let Some(gcc) = gcc_path.as_deref() {
            let mut flags: Vec<&str> = vec![
                gcc,
                "-march=rv64im",
                "-mabi=lp64",
                "-mstrict-align",
                "-falign-functions=2",
            ];
            let isystem_flag;
            if let Some(ref inc) = sp1_include_dir {
                isystem_flag = format!("-isystem{inc}");
                flags.push("-ffreestanding");
                flags.push(&isystem_flag);
            }
            builder.c_flags(&flags)
        } else {
            builder
        }
    }

    fn bins(&self, names: &[&str], dest: &str) {
        rerun_if_changed(&[]);
        let bins = self.meta.get_bins(names);
        let builder = self.builder();
        let executor = builder.build_command(&self.profile, &bins);
        println!(
            "executor: \n   ${:?}\ntargets: \n   {:?}",
            executor.cmd, executor.artifacts
        );
        if executor.artifacts.is_empty() {
            panic!("No artifacts to build");
        }
        executor
            .execute()
            .expect("Execution failed")
            .sp1_placement(dest)
            .expect("Failed to export Sp1 artifacts");
    }

    fn tests(&self, names: &[&str], dest: &str) {
        rerun_if_changed(&[]);
        let tests = self.meta.get_tests(names);
        let builder = self.builder();
        let executor = builder.test_command(&self.profile, &tests);
        println!(
            "executor: \n   ${:?}\ntargets: \n   {:?}",
            executor.cmd, executor.artifacts
        );
        if executor.artifacts.is_empty() {
            panic!("No artifacts to build");
        }
        executor
            .execute()
            .expect("Execution failed")
            .sp1_placement(dest)
            .expect("Failed to export Sp1 artifacts");
    }
}
