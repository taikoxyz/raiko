use std::env;

fn main() {
    // rerun_if_src_changed(".");
    // let out_dir_env = env::var_os("OUT_DIR").unwrap();
    // let out_dir = Path::new(&out_dir_env);
    // let kzg_raw_path = out_dir.join("kzg_settings_raw.bin");

    // println!("cargo:rerun-if-changed={}", kzg_raw_path.display());
    // println!("changed={}", kzg_raw_path.display());

    env::set_var("KZG_FILE_PATH", "../../../kzg_settings_raw.bin");

    risc0_build::embed_methods();
}


// use std::path::Path;

// fn main() {
//     // 设置输出文件路径
//     let out_dir = std::env::var("OUT_DIR").unwrap();
//     let dest_path = Path::new(&out_dir).join("methods.rs");
//     // risc0_build::embed_methods();

//     std::fs::write(dest_path, "pub const RISC0_METHODS_ELF: &[u8] = &[127];
// pub const RISC0_METHODS_ID: [u32; 8] = [3295929462, 2610146498, 4198160290, 1315106251, 542909253, 2514168315, 3864100374, 2250682698];
// pub const RISC0_METHODS_PATH: &str = \"\";").unwrap();
    
//     // 示例：打印一条消息
//     println!("Build script completed!");
// }
