use raiko_lib::signature::generate_key;
use std::{
    env::{set_var, var},
    fs::write,
    path::PathBuf,
};

const SECRET_NAME: &str = "secret.key";

fn main() {
    println!("Generating KP");
    let mut out_dir = PathBuf::from(var("OUT_DIR").unwrap());
    out_dir.push(SECRET_NAME);
    set_var("SECRET_PATH", &out_dir);
    let kp = generate_key();
    println!("Writing secret to file {}", &out_dir.to_string_lossy());
    write(out_dir, kp.secret_bytes()).unwrap();
    println!("Done");
}
