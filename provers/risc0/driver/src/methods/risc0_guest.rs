
pub const RISC0_GUEST_ELF: &[u8] = include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [405814261, 645472475, 3368860906, 1069727513, 2312368391, 2313520942, 2156489466, 779875178];
pub const RISC0_GUEST_PATH: &str = r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest"#;
