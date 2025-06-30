pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256.bin");
pub const SHA256_ID: [u32; 8] = [
    3895757990, 2237020601, 2097563348, 2436670013, 3163240250, 667496239, 1187930595, 338577159,
];
