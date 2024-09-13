pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    120620766, 3895849966, 3454466213, 2248933936, 2068139275, 3561387734, 3426824243, 3764143,
];
