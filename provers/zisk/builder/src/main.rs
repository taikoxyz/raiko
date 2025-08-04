use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("Building Zisk guest programs...");
    
    // Get the path to the guest directory
    let guest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("guest");
    
    // Build all guest programs (batch and aggregation)
    let output = Command::new("cargo-zisk")
        .args(["build", "--release"])
        .current_dir(&guest_dir)
        .output();

    match output {
        Ok(output) => {
            if !output.status.success() {
                eprintln!(
                    "Failed to build Zisk guest programs: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                std::process::exit(1);
            }
            println!("Successfully built Zisk guest programs");
        }
        Err(e) => {
            eprintln!("Failed to execute cargo-zisk: {}", e);
            eprintln!("Please ensure cargo-zisk is installed:");
            eprintln!("  TARGET=zisk make install");
            eprintln!("or manually:");
            eprintln!("  curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash");
            std::process::exit(1);
        }
    }

    println!("Zisk guest programs built successfully");
    println!("Note: ROM setup will be performed automatically during proving");
}