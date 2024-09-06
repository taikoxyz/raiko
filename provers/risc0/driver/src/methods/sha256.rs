pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    132941495, 1394354180, 2483947798, 3480748829, 3053483990, 1992985158, 3249765362, 2401282931,
];
