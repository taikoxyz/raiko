pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    2298136242, 1053664246, 2382488709, 4132520571, 996328433, 1183837582, 1858071920, 1572416462,
];
pub const ECDSA_PATH: &str =
    r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa"#;
