pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1969729193, 1889995288, 261404698, 2630336538, 339020519, 1410619780, 514721746, 1213424171,
];
