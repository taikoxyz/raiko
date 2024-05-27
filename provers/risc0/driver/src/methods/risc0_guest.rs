pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2913493253, 3394948572, 1016455208, 2248481430, 1003343387, 1683995099, 2606924676, 3573161770,
];
pub const RISC0_GUEST_PATH: &str = r#"/home/jony/projects/rust/taiko/raiko/provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest"#;
