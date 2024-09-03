pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    3154357135, 4157790813, 123789652, 116361652, 829137687, 2314522156, 1964429423, 2989684539,
];
