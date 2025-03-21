pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1689653193, 2796478021, 3874123379, 560216071, 3867155830, 2784172499, 3235388420, 507179944,
];
