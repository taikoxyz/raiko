pub const SHA256_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/sha256");
pub const SHA256_ID: [u32; 8] = [
    284623640, 3696386847, 995407058, 1839006951, 4246953846, 4005123554, 3918666326, 939004335,
];
