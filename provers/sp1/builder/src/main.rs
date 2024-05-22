use raiko_pipeline::{
    parse_metadata, rerun_if_changed, CommandBuilder, GuestMetadata, Metadata, Pipeline,
};
use std::path::PathBuf;

fn main() {
    let pipeline = Sp1Pipeline::new("provers/sp1/guest", "release");
    pipeline.bins(&["sp1-guest"], "provers/sp1/guest/elf");
    #[cfg(feature = "test")]
    pipeline.tests(&["sp1-guest"], "provers/sp1/guest/elf");
    #[cfg(feature = "bench")]
    pipeline.bins(
        &["ecdsa", "sha256", "bn254_add", "bn254_mul"],
        "provers/sp1/guest/elf",
    );
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
        CommandBuilder::new(&self.meta, "riscv32im-succinct-zkvm-elf", "succinct")
            .rust_flags(&[
                "passes=loweratomic",
                "link-arg=-Ttext=0x00200800",
                "panic=abort",
            ])
            /*.cc_compiler("gcc".into())
            .c_flags(&[
                "/opt/riscv/bin/riscv32-unknown-elf-gcc",
                "-mstrict-align",
                "-march=rv32im",
                "-falign-functions=2",
            ])
            */
            .cc_compiler("clang".into())
            .c_flags(&[
                /*"-target riscv32-unknown-elf",
                "-mstrict-align",
                "-march=rv32im",
                "-falign-functions=2",
                "--sysroot=/opt/riscv/riscv32-unknown-elf",
                "--gcc-toolchain=/opt/riscv/",*/
                "-Wstrict-aliasing",
                //"-fconserve-stack",
                "-mstrict-align",
                "-march=rv32im",
                "-falign-functions=2",
                "-DRISCV=1",
                "-mabi=ilp32",
                "-march=rv32im",
                "-ffreestanding",
                "-fno-strict-aliasing",
                "-fno-exceptions",
                "-fno-non-call-exceptions",
                //"-Wall",
                "-Wunused-but-set-parameter",
                "-Wno-error=pragmas",
                "-Wno-unknown-pragmas",
                "-Wno-strict-aliasing",
                "-isystem",
                "-fdata-sections",
                "-ffunction-sections",
                //"-findirect-inlining",
                //"-finline-small-functions",
                "-g0",
                "-O0",
                "--sysroot=/opt/riscv/riscv32-unknown-elf",
                "--gcc-toolchain=/opt/riscv/",
            ])
            .custom_args(&["--ignore-rust-version"])
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
