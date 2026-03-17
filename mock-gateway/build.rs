use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let rule_id = env::var("MOCK_RULE_ID").unwrap_or_else(|_| "example-fourth-call-error".into());
    let rules_root = env::var("MOCK_RULES_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("generated"));
    let rule_file = rules_root.join(&rule_id).join("ticket.rs");
    let fallback_rule_file = manifest_dir.join("src").join("default_rule.rs");
    let active_rule_file = if rule_file.exists() {
        rule_file
    } else {
        fallback_rule_file
    };

    println!("cargo:rerun-if-env-changed=MOCK_RULE_ID");
    println!("cargo:rerun-if-env-changed=MOCK_RULES_ROOT");
    println!("cargo:rerun-if-changed={}", active_rule_file.display());
    println!("cargo:rustc-env=MOCK_RULE_FILE={}", active_rule_file.display());
    println!("cargo:rustc-env=MOCK_RULE_ID={rule_id}");
}
