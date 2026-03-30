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
        // Prefer SP1 bundled gcc, but if it requires a newer GLIBC than the system provides
        // (detected by a failed probe run), fall back to the system-installed cross-compiler.
        let sp1_gcc_str = sp1_gcc.to_string_lossy().to_string();
        let sp1_gcc_usable = sp1_gcc.exists()
            && std::process::Command::new(&sp1_gcc)
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
        let gcc_path = if sp1_gcc_usable {
            Some(sp1_gcc_str)
        } else {
            find_on_path("riscv64-elf-gcc").or_else(|| find_on_path("riscv64-unknown-elf-gcc"))
        };

        // Discover SP1 newlib sysroot headers.
        // Layouts tried in order:
        //   ~/.sp1/riscv/riscv64-unknown-elf/include/  (new SP1 v6 layout)
        //   ~/.sp1/riscv/<platform-triple>/riscv32-unknown-elf/include/  (old layout)
        let riscv_dir = sp1_dir.join("riscv");
        let sp1_include_dir = [riscv_dir.join("riscv64-unknown-elf/include")]
            .into_iter()
            .chain(riscv_dir.read_dir().ok().into_iter().flat_map(|rd| {
                rd.filter_map(|e| {
                    let dir = e.ok()?.path();
                    Some(dir.join("riscv32-unknown-elf/include"))
                })
                .collect::<Vec<_>>()
            }))
            .find(|p| {
                p.is_dir()
                    && p.read_dir()
                        .map(|mut d| d.next().is_some())
                        .unwrap_or(false)
            })
            .map(|p| p.to_string_lossy().to_string());

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
