pub const RISC0_GUEST_ELF: &[u8] =
    include_bytes!("../../../guest/target/riscv32im-risc0-zkvm-elf/release/risc0-guest");
pub const RISC0_GUEST_ID: [u32; 8] = [
    2705224968, 672422473, 3589767632, 3895344282, 3642477750, 1142566656, 2251137472, 1131663031,
];
