pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    4259611926, 3952684265, 1745002642, 709667218, 1061040141, 2661261160, 327927262, 1889677793,
];
