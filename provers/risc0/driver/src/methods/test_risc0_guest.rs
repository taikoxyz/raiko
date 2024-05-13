
pub const TEST_RISC0_GUEST_ELF: &[u8] = include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/deps/risc0_guest-f2134d0a9dc1b778");
pub const TEST_RISC0_GUEST_ID: [u32; 8] = [3875868406, 1668916610, 1404967724, 4014172081, 1679226880, 1967164957, 1092356078, 3079385105];
pub const TEST_RISC0_GUEST_PATH: &str = r#"provers/risc0/guest/target/riscv32im-risc0-zkvm-elf/release/deps/risc0_guest-f2134d0a9dc1b778"#;
