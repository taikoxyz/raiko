pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2008664807, 702630247, 4092627615, 3260109499, 2561937414, 4278577207, 738819978, 61928151,
];
