pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2522428380, 1790994278, 397707036, 244564411, 3780865207, 1282154214, 1673205005, 3172292887,
];
