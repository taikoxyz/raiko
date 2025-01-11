pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2915820976, 1641740336, 1994238336, 1088773515, 373888610, 185287152, 956320274, 2549679061,
];
