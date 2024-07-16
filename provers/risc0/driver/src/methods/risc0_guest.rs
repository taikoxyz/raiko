pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1914784930, 3634152083, 2963332796, 2630159414, 3104046433, 3092402903, 3447446567, 3034579556,
];
