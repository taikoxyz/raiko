pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    3819323098, 64503827, 1192611593, 2785663194, 743346770, 3792505576, 3991352840, 1259868726,
];
