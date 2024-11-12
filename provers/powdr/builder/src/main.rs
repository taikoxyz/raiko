//use raiko_pipeline::{
//    parse_metadata, rerun_if_changed, CommandBuilder, GuestMetadata, Metadata, Pipeline,
//};
use std::path::PathBuf;

use powdr::Session;

fn main() {
    let mut session = Session::builder()
        .guest_path("provers/powdr/guest")
        .out_path("provers/powdr/driver/powdr-target")
        .build();

    /*
        let pipeline = PowdrPipeline::new("provers/powdr/guest", "release");
        pipeline.bins(&["powdr-guest"], "provers/powdr/driver/src/methods");
        #[cfg(feature = "test")]
        pipeline.tests(&["powdr-guest"], "provers/powdr/driver/src/methods");
        #[cfg(feature = "bench")]
        pipeline.bins(&["ecdsa", "sha256"], "provers/powdr/driver/src/methods");
    */
}

/*
pub struct PowdrPipeline {
    pub meta: Metadata,
    pub profile: String,
}

impl Pipeline for PowdrPipeline {
    fn new(root: &str, profile: &str) -> Self {
        raiko_pipeline::ROOT_DIR.get_or_init(|| PathBuf::from(root));
        PowdrPipeline {
            meta: parse_metadata(root),
            profile: profile.to_string(),
        }
    }

    fn builder(&self) -> CommandBuilder {
        let mut builder =
            CommandBuilder::new(&self.meta, "riscv32im-risc0-zkvm-elf", "nightly-2024-04-18")
                .rust_flags(&[
                    "link-arg=--emit-relocs",
                    "link-arg=-Tpowdr.x",
                    "passes=loweratomic",
                    "panic=abort",
                ])
                .cc_compiler("gcc".into())
                .c_flags(&[
                    "/opt/riscv/bin/riscv32-unknown-elf-gcc",
                    "-march=rv32im",
                    "-mstrict-align",
                    "-falign-functions=2",
                ])
                .custom_args(&[
                    "-Zbuild-std=std,panic_abort",
                    "-Zbuild-std-features=default,compiler-builtins-mem",
                ]);
        // Cannot use /.rustup/toolchains/powdr/bin/cargo, use regular cargo
        builder.unset_cargo();
        builder
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
            // Not sure what to do instead of `risc0_placement`. Maybe run
            // `powdr-rs compile` ?
            .risc0_placement(dest)
            .expect("Failed to export Powdr artifacts");
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
            .risc0_placement(dest)
            .expect("Failed to export Powdr artifacts");
    }
}
*/
