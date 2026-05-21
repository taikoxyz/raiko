use std::fs::write;
use std::path::Path;
use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=SP1_ZKVM_MAX_MEMORY");
    println!("cargo:rerun-if-env-changed=SP1_ZKVM_INPUT_REGION_SIZE");

    let target_pointer_width = env::var("CARGO_CFG_TARGET_POINTER_WIDTH").unwrap_or_default();
    let target_usize_max = match target_pointer_width.as_str() {
        "32" => u32::MAX as u64,
        _ => u64::MAX,
    };

    let default_max_memory = if target_pointer_width == "32" {
        (u32::MAX as u64) & !7
    } else {
        1u64 << 37
    };

    let default_input_region_size = if target_pointer_width == "32" {
        1u64 << 30
    } else {
        1u64 << 34
    };

    let max_memory: u64 = env::var("SP1_ZKVM_MAX_MEMORY")
        .ok()
        .map(|s| s.parse().expect("SP1_ZKVM_MAX_MEMORY must be a valid integer!"))
        .unwrap_or(default_max_memory);

    let input_region_size: u64 = env::var("SP1_ZKVM_INPUT_REGION_SIZE")
        .ok()
        .map(|s| s.parse().expect("SP1_ZKVM_INPUT_REGION_SIZE must be a valid integer!"))
        .unwrap_or(default_input_region_size);

    assert!(
        max_memory <= target_usize_max,
        "SP1_ZKVM_MAX_MEMORY must fit in target usize"
    );
    assert!(
        input_region_size <= target_usize_max,
        "SP1_ZKVM_INPUT_REGION_SIZE must fit in target usize"
    );
    assert!(
        input_region_size < max_memory,
        "SP1_ZKVM_INPUT_REGION_SIZE must be smaller than SP1_ZKVM_MAX_MEMORY"
    );

    let source = format!(
        r#"
#[allow(dead_code)]
mod configs {{
    pub const MAX_MEMORY: usize = {max_memory};
    pub const INPUT_REGION_SIZE: usize = {input_region_size};
}}
"#
    );

    let out_dir = env::var("OUT_DIR").unwrap();
    let source_path = Path::new(&out_dir).join("configs.rs");
    write(source_path, source).expect("write");
}
