pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    3652025223, 1048803843, 2950123308, 1536068232, 1159324221, 1265391242, 958811727, 4248139033,
];
