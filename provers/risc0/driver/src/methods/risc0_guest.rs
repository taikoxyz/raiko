pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest.bin");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2151630411, 3224101199, 1088151439, 676925749, 2975387509, 3045345968, 522682411, 3254761167,
];
