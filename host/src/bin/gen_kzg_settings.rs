use std::{
    fs::{self, File},
    io::Read,
    path::PathBuf,
};

use kzg::kzg_proofs::KZGSettings;
static FILE_NAME: &str = "zkcrypto_kzg_settings.bin";
const TRUSTED_SETUP_PATH: &str = "src/bin/trusted_setup.txt";

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let path = PathBuf::from(manifest)
        .join(TRUSTED_SETUP_PATH)
        .into_os_string()
        .into_string()
        .unwrap();

    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let (g1, g2) =
        kzg_traits::eip_4844::load_trusted_setup_string(&contents).expect("Failed to load points");

    let kzg_setting: KZGSettings = kzg_traits::eip_4844::load_trusted_setup_rust(&g1, &g2)
        .expect("failed to load trusted setup");

    let path = PathBuf::from("./lib/kzg_settings");
    if !path.exists() {
        fs::create_dir_all(&path).expect("Couldn't create destination directories");
    }
    let file = fs::File::create(path.join(FILE_NAME)).expect("Couldn't create file");

    bincode::serialize_into(file, &kzg_setting).expect("failed to serialize kzg settings");
}
