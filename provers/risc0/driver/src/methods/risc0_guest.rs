pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    643196937, 1246728629, 1886769928, 130762256, 177277998, 252070675, 1250330519, 622696287,
];
