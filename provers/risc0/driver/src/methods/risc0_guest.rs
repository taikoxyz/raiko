pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3473581204, 2561439051, 2320161003, 3018340632, 1481329104, 1608433297, 3314099706, 2669934765,
];
