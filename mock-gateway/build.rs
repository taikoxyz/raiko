use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let rule_id = env::var("MOCK_RULE_ID").unwrap_or_else(|_| "example-fourth-call-error".into());
    let rules_root = env::var("MOCK_RULES_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("generated"));
    let rule_file = rules_root.join(&rule_id).join("ticket.rs");

    println!("cargo:rerun-if-env-changed=MOCK_RULE_ID");
    println!("cargo:rerun-if-env-changed=MOCK_RULES_ROOT");
    println!("cargo:rerun-if-changed={}", rule_file.display());
    println!("cargo:rustc-env=MOCK_RULE_FILE={}", rule_file.display());
    println!("cargo:rustc-env=MOCK_RULE_ID={rule_id}");
}
