pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2273192126, 3371600541, 2393149482, 649684540, 896534796, 3888705079, 1226654129, 1785271921,
];
