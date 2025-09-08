use raiko_pipeline::{
    parse_metadata, rerun_if_changed, CommandBuilder, GuestMetadata, Metadata, Pipeline,
};
use std::path::PathBuf;

fn main() {
    let pipeline = ZiskPipeline::new("provers/zisk/guest", "release");
    pipeline.bins(&["zisk-aggregation", "zisk-batch"], "provers/zisk/guest/elf");
    #[cfg(feature = "test")]
    pipeline.tests(&["zisk-batch"], "provers/zisk/guest/elf");
}

pub struct ZiskPipeline {
    pub meta: Metadata,
    pub profile: String,
}

impl Pipeline for ZiskPipeline {
    fn new(root: &str, profile: &str) -> Self {
        raiko_pipeline::ROOT_DIR.get_or_init(|| PathBuf::from(root));
        ZiskPipeline {
            meta: parse_metadata(root),
            profile: profile.to_string(),
        }
    }

    fn builder(&self) -> CommandBuilder {
        CommandBuilder::new(&self.meta, "riscv64ima-zisk-zkvm-elf", "zisk")
            .rust_flags(&[
                "panic=abort",
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
            .zisk_placement(dest)
            .expect("Failed to export Zisk artifacts");
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
            .zisk_placement(dest)
            .expect("Failed to export Zisk artifacts");
    }
}