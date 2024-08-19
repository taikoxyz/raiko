pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    1030743442, 3697463329, 2083175350, 1726292372, 629109085, 444583534, 849554126, 3148184953,
];
