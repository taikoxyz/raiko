pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2653855140, 4183147639, 3898452545, 2166830558, 3298377905, 3571808978, 1262215522, 4252995592,
];
