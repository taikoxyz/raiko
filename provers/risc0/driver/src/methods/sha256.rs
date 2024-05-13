
pub const SHA256_ELF: &[u8] = include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [3212202202, 3880734562, 3977985800, 3462182722, 1762988696, 2700707388, 359464217, 3702618422];
pub const SHA256_PATH: &str = r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/sha256"#;
