pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa.bin");
pub const ECDSA_ID: [u32; 8] = [
    2179975382, 802267811, 3940651088, 613491399, 186235302, 2489399115, 3970096106, 2637606398,
];
