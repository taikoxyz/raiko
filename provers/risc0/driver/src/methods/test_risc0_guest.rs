
pub const TEST_RISC0_GUEST_ELF: &[u8] = include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/deps/risc0_guest-0fbb3c3c4c4e6748");
pub const TEST_RISC0_GUEST_ID: [u32; 8] = [1202766389, 421468472, 3745726180, 79799317, 444303592, 3187291209, 506679673, 2669770220];
pub const TEST_RISC0_GUEST_PATH: &str = r#"provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/deps/risc0_guest-0fbb3c3c4c4e6748"#;
