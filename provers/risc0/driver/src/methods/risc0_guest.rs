pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3976224200, 4029454189, 3348261788, 2475843373, 2147201141, 4142012932, 2343594982, 1511845164,
];
pub const RISC0_GUEST_PATH: &str =
    r#"/home/ubuntu/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest"#;
