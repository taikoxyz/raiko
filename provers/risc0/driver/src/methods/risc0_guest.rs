pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    465583552, 1408605088, 3606449196, 1642669483, 2242147717, 224244690, 1323689776, 2289565713,
];
