pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    2056184419, 2138278279, 1402585036, 1124855978, 1272938995, 187539054, 1800814138, 2227164774,
];
pub const SHA256_PATH: &str =
    r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/sha256"#;
