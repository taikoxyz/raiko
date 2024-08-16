pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    1166688769, 1407190737, 3347938864, 1261472884, 3997842354, 3752365982, 4108615966, 2506107654,
];
