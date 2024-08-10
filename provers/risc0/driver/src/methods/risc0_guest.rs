pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1976895882, 1330368590, 4103886380, 1528062801, 1566791114, 1718329952, 3941843852, 3989505347,
];
