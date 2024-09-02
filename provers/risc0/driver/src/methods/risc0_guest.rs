pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3792684981, 3631776425, 722746971, 3865806131, 1697935989, 4271189554, 4091571985, 3318206029,
];
