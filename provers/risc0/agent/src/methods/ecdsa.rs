pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa.bin");
pub const ECDSA_ID: [u32; 8] = [
    392772736, 1212292806, 891633355, 1757481832, 2310878719, 4186565357, 1383043300, 1895477953,
];
