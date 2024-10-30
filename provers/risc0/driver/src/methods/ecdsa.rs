pub const ECDSA_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/ecdsa");
pub const ECDSA_ID: [u32; 8] = [
    46843261, 4287341384, 1164714702, 1381776748, 1542613440, 1347970650, 2481906212, 2285212198,
];
