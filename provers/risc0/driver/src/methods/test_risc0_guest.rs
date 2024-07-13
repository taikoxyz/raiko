pub const TEST_RISC0_GUEST_ELF: &[u8] = include_bytes!(
    "../../../guest/target/riscv32im-risc0-zkvm-elf/release/deps/risc0_guest"
);
pub const TEST_RISC0_GUEST_ID: [u32; 8] = [
    1938439720, 3200608207, 3901424709, 2031136574, 850058303, 3058786714, 2432413607, 2771178900,
];
