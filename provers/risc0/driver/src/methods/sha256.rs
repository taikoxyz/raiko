pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    3506084161, 1146489446, 485833862, 3404354046, 3626029993, 1928006034, 3833244069, 3073098029,
];
