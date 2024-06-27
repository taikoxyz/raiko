pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1651774355, 734744405, 3018462910, 4078349899, 1233289727, 863040824, 1806845319, 2266402868,
];
pub const RISC0_GUEST_PATH: &str =
    r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest"#;
