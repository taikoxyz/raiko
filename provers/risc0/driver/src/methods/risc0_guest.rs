pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    1848002361, 3447634449, 2932177819, 2827220601, 4284138344, 2572487667, 1602600202, 3769687346,
];
