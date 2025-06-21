pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest.bin");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1189949973, 706254424, 3202843234, 1301656159, 1293917122, 1522777751, 3989630166, 3326741603,
];
