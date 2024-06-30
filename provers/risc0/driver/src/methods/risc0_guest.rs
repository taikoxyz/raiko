pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1106284168, 2121988569, 4030233466, 422073515, 2026351144, 1404012643, 690639581, 3252900346,
];
pub const RISC0_GUEST_PATH: &str =
    r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest"#;
