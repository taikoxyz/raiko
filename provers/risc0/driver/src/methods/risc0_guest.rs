pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3141167402, 1225770980, 1142441850, 726484406, 3323311801, 4277256420, 1757388566, 240273689,
];
