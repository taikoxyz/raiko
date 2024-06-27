#![feature(slice_flatten)]
use std::{fs, path::PathBuf};

use reth_primitives::revm_primitives::kzg::{G1Points, G2Points, G1_POINTS, G2_POINTS};
cfg_if::cfg_if! {
    if #[cfg(feature = "kzg-zkcrypto")] {
        use rust_kzg_zkcrypto::kzg_proofs::KZGSettings as TaikoKzgSettings;
        static FILE_NAME: &str = "zkcrypto_kzg_settings.bin";
    }
}

fn main() {
    let kzg_setting: TaikoKzgSettings = kzg::eip_4844::load_trusted_setup_rust(
        G1Points::as_ref(G1_POINTS).flatten(),
        G2Points::as_ref(G2_POINTS).flatten(),
    )
    .expect("failed to load trusted setup");

    let path = PathBuf::from("./lib/kzg_settings");
    if !path.exists() {
        fs::create_dir_all(&path).expect("Couldn't create destination directories");
    }
    let file = fs::File::create(path.join(FILE_NAME)).expect("Couldn't create file");

    bincode::serialize_into(file, &kzg_setting).expect("failed to serialize kzg settings");
}
