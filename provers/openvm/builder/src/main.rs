use raiko_pipeline::{parse_metadata, rerun_if_changed, Metadata, Pipeline};
use std::{
    path::PathBuf,
    process::Command,
};

fn main() {
    let pipeline = OpenVMPipeline::new("provers/openvm/guest", "release");
    pipeline.bins(
        &["openvm-aggregation", "openvm-batch"],
        "provers/openvm/driver/src/methods",
    );
}

pub struct OpenVMPipeline {
    pub meta: Metadata,
    pub profile: String,
}

impl Pipeline for OpenVMPipeline {
    fn new(root: &str, profile: &str) -> Self {
        raiko_pipeline::ROOT_DIR.get_or_init(|| PathBuf::from(root));
        OpenVMPipeline {
            meta: parse_metadata(root),
            profile: profile.to_string(),
        }
    }

    fn builder(&self) -> raiko_pipeline::CommandBuilder {
        // This method is not used for OpenVM since we use cargo-openvm directly
        raiko_pipeline::CommandBuilder::new(&self.meta, "riscv32im-risc0-zkvm-elf", "openvm")
    }

    fn bins(&self, names: &[&str], dest: &str) {
        rerun_if_changed(&[]);

        let root = raiko_pipeline::ROOT_DIR.get().expect("No reference to ROOT_DIR");

        println!("\n===========================================");
        println!("Building OpenVM guest programs");
        println!("===========================================");
        println!("Working directory: {:?}", root);
        println!("Target: riscv32im-risc0-zkvm-elf");
        println!("Profile: {}", self.profile);
        println!("Output directory: {}", dest);

        // Build all binaries at once using cargo openvm build
        // The RISC-V target is determined by the OpenVM configuration (openvm.toml)
        let mut cmd = Command::new("cargo");
        cmd.arg("openvm")
            .arg("build")
            .arg("--bins")  // Build all binaries at once
            .arg("--profile")
            .arg(&self.profile)
            .current_dir(root);

        println!("\nExecuting: {:?}", cmd);
        println!("===========================================\n");

        let output = cmd
            .output()
            .expect("Failed to execute cargo openvm build");

        // Print stdout and stderr for debugging
        if !output.stdout.is_empty() {
            println!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        }

        if !output.status.success() {
            panic!("cargo openvm build failed with status: {}", output.status);
        }

        // Copy ELF files to destination
        println!("\n===========================================");
        println!("Copying ELF binaries to driver");
        println!("===========================================");

        // OpenVM outputs to target/openvm/ directory, not the RISC-V target directory
        let target_dir = self.meta.target_directory.join("openvm");

        println!("Source directory: {:?}", target_dir);

        for bin_name in names {
            let src = target_dir.join(bin_name);
            let dest_file = PathBuf::from(dest).join(bin_name);

            if !src.exists() {
                panic!("ELF binary not found: {:?}\nMake sure 'cargo openvm build' completed successfully.", src);
            }

            println!("  {} -> {}", src.as_str(), dest_file.display());

            std::fs::copy(&src, &dest_file)
                .unwrap_or_else(|e| panic!("Failed to copy {} to {}: {}", src.as_str(), dest_file.display(), e));
        }

        println!("\n===========================================");
        println!("OpenVM build complete!");
        println!("===========================================\n");
    }

    fn tests(&self, _names: &[&str], _dest: &str) {
        unimplemented!("OpenVM tests not yet implemented");
    }
}
